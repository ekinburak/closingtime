#!/usr/bin/env python3
"""Verify terminal behavior and interactive approval using only bounded, owned fixtures."""
import argparse
import errno
import json
import os
import pty
import select
import signal
import subprocess
import sys
import tempfile
import time


def terminal(command, trigger, reply, redirect_input=False):
    pid, descriptor = pty.fork()
    if pid == 0:
        if redirect_input:
            null = os.open(os.devnull, os.O_RDONLY)
            os.dup2(null, 0)
            os.close(null)
        os.execv(command[0], command)
    output = bytearray()
    replied = False
    deadline = time.monotonic() + 12
    reaped = False
    try:
        while time.monotonic() < deadline:
            readable, _, _ = select.select([descriptor], [], [], 0.05)
            if readable:
                try:
                    data = os.read(descriptor, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b""
                if data:
                    output.extend(data)
                    if not replied and trigger in output:
                        os.write(descriptor, reply)
                        replied = True
            child, status = os.waitpid(pid, os.WNOHANG)
            if child:
                reaped = True
                return os.waitstatus_to_exitcode(status), output.decode(errors="replace"), replied
        raise AssertionError("terminal fixture timed out: " + output.decode(errors="replace"))
    finally:
        if not reaped:
            # This is our unreaped direct test child, not a searched-for user process.
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        os.close(descriptor)


parser = argparse.ArgumentParser()
parser.add_argument("--binary", default="target/debug/closingtime")
args = parser.parse_args()
binary = os.path.abspath(args.binary)

with tempfile.TemporaryDirectory(prefix="closingtime-terminal-") as directory:
    state = os.path.join(directory, "state")
    prefix = [binary, "--state-dir", state]
    counter = """
import signal,time
hits=[]
signal.signal(signal.SIGINT,lambda *_: hits.append(1))
print('READY',flush=True)
end=time.monotonic()+1.5
while time.monotonic()<end: time.sleep(.02)
print('COUNT='+str(len(hits)),flush=True)
raise SystemExit(0 if len(hits)==1 else 9)
"""
    for redirect_input in [False, True]:
        code, output, replied = terminal(prefix + ["run", "--", sys.executable, "-c", counter], b"READY", b"\x03", redirect_input)
        assert code == 0 and replied and "COUNT=1" in output, output
        print("Terminal Ctrl-C reaches the wrapped command once (redirected stdin=%s): passed" % redirect_input)

    ready = os.path.join(directory, "ready.json")
    worker = """
import json,os,socket,sys,time
s=socket.socket();s.bind(('127.0.0.1',0));s.listen()
with open(sys.argv[1],'w') as f: json.dump({'pid':os.getpid(),'port':s.getsockname()[1]},f)
time.sleep(25)
"""
    launcher = """
import subprocess,sys,time
subprocess.Popen([sys.executable,'-c',sys.argv[1],sys.argv[2]],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
time.sleep(.7)
"""
    result = subprocess.run(prefix + ["run", "--label", "approval fixture", "--", sys.executable, "-c", launcher, worker, ready], capture_output=True, text=True, timeout=10)
    assert result.returncode == 0, result.stderr
    with open(ready) as source:
        resource = json.load(source)
    export = json.loads(subprocess.check_output(prefix + ["export", "--json"]))
    session = next(s for s in export["sessions"] if s["label"] == "approval fixture")
    preview = json.loads(subprocess.check_output(prefix + ["clean", "--session", session["id"], "--json"]))
    assert any(r["identity"]["pid"] == resource["pid"] and r["cleanup_eligible"] for r in preview["resources"])
    code, output, replied = terminal(prefix + ["clean", "--session", session["id"], "--apply"], b"Type yes:", b"yes\n")
    assert code == 0 and replied and "stopped_or_awaiting_reaping" in output, output
    export = json.loads(subprocess.check_output(prefix + ["export", "--json"]))
    assert any(a["identity"]["pid"] == resource["pid"] and a["finished_ms"] for a in export["actions"])
    print("Interactive cleanup stops its reviewed fixture and records the outcome: passed")
