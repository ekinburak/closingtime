#!/usr/bin/env python3
"""Read Closingtime's public export without inspecting SQLite or applying cleanup."""
import argparse
import json
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--binary", default="closingtime")
parser.add_argument("--state-dir")
args = parser.parse_args()
command = [args.binary]
if args.state_dir:
    command += ["--state-dir", args.state_dir]
command += ["export", "--json"]
data = json.loads(subprocess.run(command, check=True, capture_output=True, text=True).stdout)
if data.get("schema") != "closingtime.session.v1":
    raise SystemExit("Unsupported Closingtime export schema")
for session in data["sessions"]:
    records = [record for record in data["processes"] if record["session_id"] == session["id"]]
    print(json.dumps({"run": session["id"], "state": session["state"], "recorded_processes": len(records)}))
