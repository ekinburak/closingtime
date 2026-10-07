//! Real OS fixtures, confined to processes launched by this test. Workers expire after 30s.
use closingtime_core::*;
use std::{
    fs,
    io::Write,
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("closingtime-native-{}", new_id().unwrap())))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Execute this same test executable as a controlled root/middle/server process.
#[test]
fn fixture_worker() {
    let Ok(mode) = std::env::var("CLOSINGTIME_FIXTURE_MODE") else {
        return;
    };
    let path = PathBuf::from(std::env::var_os("CLOSINGTIME_FIXTURE_READY").unwrap());
    if mode == "server" || mode == "ignore-term" {
        if mode == "ignore-term" {
            unsafe {
                libc::signal(libc::SIGTERM, libc::SIG_IGN);
            }
        }
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut f = fs::File::create(&path).unwrap();
        writeln!(
            f,
            "{} {}",
            std::process::id(),
            server.local_addr().unwrap().port()
        )
        .unwrap();
        f.sync_all().unwrap();
        std::thread::sleep(Duration::from_secs(30));
        drop(server);
    } else if mode == "middle" {
        let mut command = worker_command("server", &path);
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        // Deliberate orphan fixture: the intermediate parent must exit before this child.
        #[allow(
            clippy::zombie_processes,
            reason = "bounded double-fork fixture intentionally reparents its child"
        )]
        let _child = command.spawn().unwrap();
    } else {
        let mut child = worker_command("middle", &path).spawn().unwrap();
        child.wait().unwrap();
        wait_ready(&path);
        std::thread::sleep(Duration::from_millis(600));
    }
}
fn worker_command(mode: &str, path: &std::path::Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([
        "--exact",
        "fixture_worker",
        "--nocapture",
        "--test-threads=1",
    ])
    .env("CLOSINGTIME_FIXTURE_MODE", mode)
    .env("CLOSINGTIME_FIXTURE_READY", path)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    c
}
fn wait_ready(path: &std::path::Path) -> (u32, u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(s) = fs::read_to_string(path) {
            let parts: Vec<_> = s.split_whitespace().collect();
            if parts.len() == 2 {
                if let (Ok(pid), Ok(port)) = (parts[0].parse(), parts[1].parse()) {
                    return (pid, port);
                }
            }
        }
        assert!(Instant::now() < deadline, "fixture did not become ready");
        std::thread::sleep(Duration::from_millis(20));
    }
}
struct FixtureGuard {
    identity: ProcessIdentity,
}
impl Drop for FixtureGuard {
    fn drop(&mut self) {
        if let Ok(backend) = NativeBackend::new() {
            let _ = backend.signal(&self.identity, libc::SIGKILL);
        }
    }
}
fn guard(backend: &NativeBackend, pid: u32) -> FixtureGuard {
    match backend.inspect(pid) {
        Inspection::Present(p) => FixtureGuard {
            identity: p.identity,
        },
        other => panic!("cannot inspect own fixture: {other:?}"),
    }
}

#[test]
fn real_double_fork_port_attribution_preview_and_cleanup() {
    let dir = TempDir::new();
    let ready = dir.0.with_extension("ready");
    let engine = Engine::new(
        Store::open(&dir.0, true).unwrap(),
        NativeBackend::new().unwrap(),
    );
    let session = engine
        .begin_session(
            "double-fork",
            "/test/project",
            Some("native-fixture".into()),
        )
        .unwrap();
    let mut root = engine
        .spawn_owned(&session.id, &mut worker_command("root", &ready))
        .unwrap();
    let (pid, port) = wait_ready(&ready);
    let _guard = guard(&engine.backend, pid);
    engine.observe().unwrap();
    assert!(root.wait().unwrap().success());
    engine.observe().unwrap();
    engine
        .end_session(&session.id, "fixture completed")
        .unwrap();
    let who = engine.who_port(port).unwrap();
    let resource = who
        .resources
        .iter()
        .find(|r| r.identity.pid == pid)
        .expect("TCP port should resolve to its fixture process");
    assert_eq!(resource.session_id.as_deref(), Some(session.id.as_str()));
    assert!(resource.cleanup_eligible, "{:?}", resource.reasons);
    assert!(resource.evidence.contains(&Evidence::InheritedTag));
    let before = serde_json::to_string(&engine.store.export().unwrap()).unwrap();
    let plan = engine.plan_cleanup(&session.id).unwrap();
    assert_eq!(
        before,
        serde_json::to_string(&engine.store.export().unwrap()).unwrap()
    );
    assert!(matches!(
        engine.backend.inspect(pid),
        Inspection::Present(_)
    ));
    let actions = engine
        .apply_plan(&plan, ReviewedApproval::for_plan(&plan))
        .unwrap();
    assert!(
        actions
            .iter()
            .any(|a| a.identity.pid == pid && a.result == "stopped_or_awaiting_reaping")
    );
    assert!(
        !engine
            .who_port(port)
            .unwrap()
            .resources
            .iter()
            .any(|r| r.identity.pid == pid)
    );
    let _ = fs::remove_file(ready);
}

#[test]
fn real_sigterm_ignored_escalates_only_the_reviewed_fixture() {
    let dir = TempDir::new();
    let ready = dir.0.with_extension("ready");
    let engine = Engine::new(
        Store::open(&dir.0, true).unwrap(),
        NativeBackend::new().unwrap(),
    );
    let s = engine.begin_session("ignore-term", "/test", None).unwrap();
    let mut root = engine
        .spawn_owned(&s.id, Command::new("/bin/sleep").arg("0.3"))
        .unwrap();
    let mut worker = worker_command("ignore-term", &ready);
    worker.env(SESSION_ENV, &s.id);
    let mut child = worker.spawn().unwrap();
    let (pid, _) = wait_ready(&ready);
    let _guard = guard(&engine.backend, pid);
    engine.register_owned(&s.id, pid).unwrap();
    root.wait().unwrap();
    engine.end_session(&s.id, "root done").unwrap();
    let plan = engine.plan_cleanup(&s.id).unwrap();
    assert!(
        plan.resources
            .iter()
            .any(|r| r.identity.pid == pid && r.cleanup_eligible)
    );
    let actions = engine
        .apply_plan(&plan, ReviewedApproval::for_plan(&plan))
        .unwrap();
    let action = actions.iter().find(|a| a.identity.pid == pid).unwrap();
    assert_eq!(action.signals, vec!["SIGTERM", "SIGKILL"]);
    child.wait().unwrap();
    let _ = fs::remove_file(ready);
}

#[test]
fn cli_wrapper_preserves_exit_status_and_rejects_piped_apply() {
    let dir = TempDir::new();
    let binary = env!("CARGO_BIN_EXE_closingtime");
    let output = Command::new(binary)
        .arg("--state-dir")
        .arg(&dir.0)
        .args(["run", "--", "/bin/sh", "-c", "exit 17"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(17),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let export = Command::new(binary)
        .arg("--state-dir")
        .arg(&dir.0)
        .args(["export", "--json"])
        .output()
        .unwrap();
    assert!(export.status.success());
    let data: Export = serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(data.sessions.len(), 1);
    assert_eq!(data.sessions[0].state, SessionState::Ended);
    let denied = Command::new(binary)
        .arg("--state-dir")
        .arg(&dir.0)
        .args(["clean", "--session", &data.sessions[0].id, "--apply"])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("noninteractive cleanup is refused"));
    assert!(data.actions.is_empty());
    let id = &data.sessions[0].id;
    for selector in [vec!["--last"], vec!["--session", &id[..8]]] {
        let preview = Command::new(binary)
            .arg("--state-dir")
            .arg(&dir.0)
            .arg("clean")
            .args(&selector)
            .output()
            .unwrap();
        assert!(
            preview.status.success(),
            "{}",
            String::from_utf8_lossy(&preview.stderr)
        );
        assert!(String::from_utf8_lossy(&preview.stdout).contains(id.as_str()));
    }
    let recover = Command::new(binary)
        .arg("--state-dir")
        .arg(&dir.0)
        .args(["recover", "--last"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&recover.stderr).contains("already ended"));
    let missing = Command::new(binary)
        .arg("--state-dir")
        .arg(&dir.0)
        .args(["clean", "--session", "zz"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no recorded run matches zz"));
}

#[test]
fn cli_reads_without_a_ledger_create_no_files() {
    let dir = TempDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_closingtime"))
        .arg("--state-dir")
        .arg(&dir.0)
        .args(["sessions", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.0.exists());
}

// These are our unreaped direct children. They also expire if the test harness crashes.
struct OwnedChildren(Vec<std::process::Child>);
impl Drop for OwnedChildren {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn native_list_and_lookup_with_200_live_recorded_processes() {
    let dir = TempDir::new();
    let engine = Engine::new(
        Store::open(&dir.0, true).unwrap(),
        NativeBackend::new().unwrap(),
    );
    let session = engine.begin_session("latency", "/test", None).unwrap();
    let mut children = OwnedChildren(Vec::new());
    for _ in 0..200 {
        children.0.push(
            engine
                .spawn_owned(&session.id, Command::new("/bin/sleep").arg("30"))
                .unwrap(),
        );
    }
    let start = Instant::now();
    let scan = engine.scan(Some(&session.id)).unwrap();
    let list_elapsed = start.elapsed();
    assert_eq!(scan.resources.len(), 200);
    assert!(scan.resources.iter().all(|r| r.status == "running"));
    let start = Instant::now();
    let lookup = engine.who_pid(children.0[199].id()).unwrap();
    let lookup_elapsed = start.elapsed();
    assert_eq!(lookup.resources.len(), 1);
    println!(
        "200 live recorded processes: list={}ms lookup={}ms",
        list_elapsed.as_millis(),
        lookup_elapsed.as_millis()
    );
    assert!(
        list_elapsed < Duration::from_secs(1),
        "list: {list_elapsed:?}"
    );
    assert!(
        lookup_elapsed < Duration::from_secs(1),
        "lookup: {lookup_elapsed:?}"
    );
}
