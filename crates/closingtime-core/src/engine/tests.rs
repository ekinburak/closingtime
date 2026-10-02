use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("closingtime-test-{}", new_id().unwrap())))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct FakeBackend {
    state: Arc<Mutex<Snapshot>>,
    signals: Arc<Mutex<Vec<(ProcessIdentity, i32)>>>,
    uid: u32,
    refuse_signal: bool,
}
impl FakeBackend {
    fn new() -> Self {
        let uid = unsafe { libc::geteuid() };
        let mut snapshot = Snapshot::default();
        snapshot
            .processes
            .insert(std::process::id(), process(std::process::id(), uid, None));
        Self {
            state: Arc::new(Mutex::new(snapshot)),
            signals: Arc::new(Mutex::new(Vec::new())),
            uid,
            refuse_signal: false,
        }
    }
    fn put(&self, p: Process) {
        self.state
            .lock()
            .unwrap()
            .processes
            .insert(p.identity.pid, p);
    }
    fn remove(&self, pid: u32) {
        self.state.lock().unwrap().processes.remove(&pid);
    }
    fn mutate(&self, pid: u32, f: impl FnOnce(&mut Process)) {
        f(self.state.lock().unwrap().processes.get_mut(&pid).unwrap());
    }
}
impl Backend for FakeBackend {
    fn host(&self) -> &str {
        "host"
    }
    fn boot(&self) -> &str {
        "boot"
    }
    fn uid(&self) -> u32 {
        self.uid
    }
    fn inspect(&self, pid: u32) -> Inspection {
        let s = self.state.lock().unwrap();
        if let Some(reason) = s.unavailable.get(&pid) {
            return Inspection::Unavailable(reason.clone());
        }
        s.processes
            .get(&pid)
            .cloned()
            .map(Inspection::Present)
            .unwrap_or(Inspection::Gone)
    }
    fn snapshot(&self) -> Result<Snapshot> {
        Ok(self.state.lock().unwrap().clone())
    }
    fn signal(&self, id: &ProcessIdentity, signal: i32) -> Result<()> {
        if self.refuse_signal {
            return Err("pidfd unavailable".into());
        }
        match self.inspect(id.pid) {
            Inspection::Present(p) if p.identity == *id => {}
            _ => return Err("changed identity".into()),
        }
        self.signals.lock().unwrap().push((id.clone(), signal));
        self.remove(id.pid);
        Ok(())
    }
}
fn process(pid: u32, uid: u32, tag: Option<&str>) -> Process {
    Process {
        identity: ProcessIdentity {
            host: "host".into(),
            boot: "boot".into(),
            uid,
            pid,
            start: format!("{pid}"),
        },
        parent_pid: 1,
        executable: Some("/fixture/server".into()),
        name: "fixture".into(),
        zombie: false,
        session_tag: tag.map(str::to_owned),
        tag_readable: true,
        manager_owned: Some(false),
    }
}
fn setup() -> (TempDir, Engine<FakeBackend>, Session) {
    let dir = TempDir::new();
    let backend = FakeBackend::new();
    let engine = Engine::new(Store::open(&dir.0, true).unwrap(), backend);
    let session = engine
        .begin_session("fixture", "/same/project", None)
        .unwrap();
    (dir, engine, session)
}
fn record(engine: &Engine<FakeBackend>, session: &Session, pid: u32) -> Process {
    let p = process(pid, engine.backend.uid, Some(&session.id));
    engine.backend.put(p.clone());
    engine.register_owned(&session.id, pid).unwrap();
    p
}

#[test]
fn labelled_safety_corpus_240_cases() {
    let (_dir, engine, _initial_session) = setup();
    // 20 independently instantiated examples of each of 12 failure/positive categories.
    let mut expected = BTreeMap::new();
    let mut records = Vec::new();
    for index in 0..240 {
        let session = engine
            .begin_session(&format!("case-{index}"), "/same/project", None)
            .unwrap();
        let pid = 1_000_000 + index;
        let mut p = process(pid, engine.backend.uid, Some(&session.id));
        let mut r = OwnershipRecord {
            identity: p.identity.clone(),
            session_id: session.id.clone(),
            parent_key: None,
            executable: p.executable.clone(),
            name: p.name.clone(),
            evidence: vec![Evidence::InheritedTag],
            first_seen_ms: now_ms(),
            last_seen_ms: now_ms(),
            conflict: false,
            manager_owned: false,
        };
        let category = index % 12;
        match category {
            0 => {} // Eligible tagged, recorded, ended resource.
            1 => {
                engine.store.set_keep(&p.identity.key(), true).unwrap();
            }
            2 => {
                p.identity.start.push_str("-reused");
            }
            3 => {
                r.identity.boot = "old-boot".into();
            }
            4 => {
                p.identity.uid += 1;
                r.identity = p.identity.clone();
            }
            5 => {
                p.tag_readable = false;
            }
            6 => {
                p.zombie = true;
            }
            7 => {
                p.manager_owned = Some(true);
            }
            8 => {
                p.manager_owned = None;
            }
            9 => {
                p.session_tag = Some("other-session".into());
            }
            10 => {
                r.conflict = true;
            }
            11 => {
                p.executable = Some("/new/executable".into());
            }
            _ => unreachable!(),
        }
        expected.insert(r.identity.key(), category == 0);
        engine.backend.put(p);
        records.push(r);
        engine.end_session(&session.id, "fixture ended").unwrap();
    }
    engine.store.save_records(&records).unwrap();
    let scan = engine.scan(None).unwrap();
    let labelled: Vec<_> = scan
        .resources
        .iter()
        .filter(|r| expected.contains_key(&r.identity.key()))
        .collect();
    assert_eq!(labelled.len(), 240);
    assert!(
        scan.resources
            .iter()
            .filter(|r| !expected.contains_key(&r.identity.key()))
            .all(|r| !r.cleanup_eligible)
    );
    let wrong: Vec<_> = labelled
        .iter()
        .filter(|r| r.cleanup_eligible != expected[&r.identity.key()])
        .collect();
    assert!(wrong.is_empty(), "incorrect decisions: {wrong:?}");
    assert_eq!(
        scan.resources.iter().filter(|r| r.cleanup_eligible).count(),
        20
    );
    assert!(engine.backend.signals.lock().unwrap().is_empty());
}

#[test]
fn same_project_sessions_and_double_fork_tags_stay_separate() {
    let (_dir, engine, a) = setup();
    let b = engine
        .begin_session("second", "/same/project", Some("native-2".into()))
        .unwrap();
    let pa = record(&engine, &a, 1_100_000);
    let pb = record(&engine, &b, 1_100_001);
    // Reparented descendants retain inherited tags without using cwd as ownership.
    engine
        .backend
        .put(process(1_100_002, engine.backend.uid, Some(&a.id)));
    engine
        .backend
        .put(process(1_100_003, engine.backend.uid, Some(&b.id)));
    engine.observe().unwrap();
    let records = engine.store.records().unwrap();
    assert_eq!(records.iter().filter(|r| r.session_id == a.id).count(), 2);
    assert_eq!(records.iter().filter(|r| r.session_id == b.id).count(), 2);
    assert_ne!(pa.identity, pb.identity);
    assert_eq!(
        engine
            .store
            .session(&b.id)
            .unwrap()
            .native_session_id
            .as_deref(),
        Some("native-2")
    );
}

#[test]
fn keep_descendants_survives_reparenting_and_future_observation() {
    let (_dir, engine, s) = setup();
    let parent = record(&engine, &s, 1_200_000);
    engine.set_keep(parent.identity.pid, true).unwrap();
    let mut child = process(1_200_001, engine.backend.uid, Some(&s.id));
    child.parent_pid = parent.identity.pid;
    engine.backend.put(child);
    engine.observe().unwrap();
    engine.backend.mutate(1_200_001, |p| p.parent_pid = 1);
    engine.observe().unwrap();
    engine.end_session(&s.id, "done").unwrap();
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| r.kept && !r.cleanup_eligible)
    );
    engine.set_keep(parent.identity.pid, false).unwrap();
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| r.cleanup_eligible)
    );
}

#[test]
fn manual_unknown_unrecorded_and_active_processes_are_not_actions() {
    let (_dir, engine, s) = setup();
    record(&engine, &s, 1_300_000);
    engine
        .backend
        .put(process(1_300_001, engine.backend.uid, None));
    engine
        .backend
        .put(process(1_300_002, engine.backend.uid, Some(&s.id)));
    assert_eq!(
        engine.who_pid(1_300_001).unwrap().resources[0].status,
        "unknown"
    );
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| !r.cleanup_eligible)
    );
    engine.end_session(&s.id, "done").unwrap();
    let plan = engine.plan_cleanup(&s.id).unwrap();
    assert!(
        plan.resources
            .iter()
            .find(|r| r.identity.pid == 1_300_002)
            .is_some_and(|r| r.status == "unrecorded" && !r.cleanup_eligible)
    );
    assert!(
        !engine
            .store
            .records()
            .unwrap()
            .iter()
            .any(|r| r.identity.pid == 1_300_002)
    );
}

#[test]
fn changed_identity_keep_and_reactivated_run_invalidate_reviewed_plan() {
    for variant in 0..3 {
        let (_dir, engine, s) = setup();
        let p = record(&engine, &s, 1_400_000);
        engine.end_session(&s.id, "done").unwrap();
        let plan = engine.plan_cleanup(&s.id).unwrap();
        assert!(plan.resources[0].cleanup_eligible);
        match variant {
            0 => engine
                .backend
                .mutate(p.identity.pid, |p| p.identity.start = "new-start".into()),
            1 => engine.set_keep(p.identity.pid, true).unwrap(),
            2 => {
                let mut s = engine.store.session(&s.id).unwrap();
                s.state = SessionState::Active;
                engine.store.save_session(&s).unwrap();
            }
            _ => unreachable!(),
        }
        let actions = engine
            .apply_plan(&plan, ReviewedApproval::for_plan(&plan))
            .unwrap();
        assert_eq!(actions[0].result, "skipped_after_recheck");
        assert!(engine.backend.signals.lock().unwrap().is_empty());
    }
}

#[test]
fn missing_end_root_unreadable_and_false_end_block_cleanup() {
    let (_dir, engine, mut s) = setup();
    let root = record(&engine, &s, 1_500_000);
    record(&engine, &s, 1_500_001);
    s.root = Some(root.identity.clone());
    engine.store.save_session(&s).unwrap();
    engine.backend.remove(root.identity.pid);
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| !r.cleanup_eligible)
    );
    engine.end_session(&s.id, "done").unwrap();
    engine
        .backend
        .state
        .lock()
        .unwrap()
        .unavailable
        .insert(root.identity.pid, "permission denied".into());
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| !r.cleanup_eligible)
    );
    engine.backend.state.lock().unwrap().unavailable.clear();
    engine.backend.put(root);
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| !r.cleanup_eligible)
    );
}

#[test]
fn clean_preview_and_export_do_not_change_ledger() {
    let (dir, engine, s) = setup();
    record(&engine, &s, 1_600_000);
    engine.end_session(&s.id, "done").unwrap();
    let before = serde_json::to_string(&engine.store.export().unwrap()).unwrap();
    let reader = Engine::new(Store::open(&dir.0, false).unwrap(), engine.backend.clone());
    reader.plan_cleanup(&s.id).unwrap();
    reader.scan(None).unwrap();
    reader.who_pid(1_600_000).unwrap();
    assert_eq!(
        before,
        serde_json::to_string(&reader.store.export().unwrap()).unwrap()
    );
    assert!(reader.set_keep(1_600_000, true).is_err());
}

#[test]
fn failed_audit_write_prevents_signals_and_pending_action_is_durable() {
    let (_dir, engine, s) = setup();
    record(&engine, &s, 1_700_000);
    engine.end_session(&s.id, "done").unwrap();
    let plan = engine.plan_cleanup(&s.id).unwrap();
    engine.store.connection.execute_batch("CREATE TRIGGER deny_action BEFORE INSERT ON actions BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END;").unwrap();
    assert!(
        engine
            .apply_plan(&plan, ReviewedApproval::for_plan(&plan))
            .is_err()
    );
    assert!(engine.backend.signals.lock().unwrap().is_empty());
}

#[test]
fn action_results_and_evidence_are_exported() {
    let (_dir, engine, s) = setup();
    record(&engine, &s, 1_800_000);
    engine.end_session(&s.id, "done").unwrap();
    let plan = engine.plan_cleanup(&s.id).unwrap();
    let actions = engine
        .apply_plan(&plan, ReviewedApproval::for_plan(&plan))
        .unwrap();
    assert_eq!(actions[0].signals, vec!["SIGTERM"]);
    assert_eq!(actions[0].result, "stopped_or_awaiting_reaping");
    let export = engine.store.export().unwrap();
    assert_eq!(export.schema, SCHEMA);
    assert_eq!(export.actions.len(), 1);
    assert!(export.actions[0].finished_ms.is_some());
}

#[test]
fn unavailable_pidfd_does_not_fall_back_to_pid_kill() {
    let (_dir, mut engine, s) = setup();
    record(&engine, &s, 1_850_000);
    engine.end_session(&s.id, "done").unwrap();
    engine.backend.refuse_signal = true;
    let plan = engine.plan_cleanup(&s.id).unwrap();
    let actions = engine
        .apply_plan(&plan, ReviewedApproval::for_plan(&plan))
        .unwrap();
    assert!(actions[0].result.starts_with("refused"));
    assert!(engine.backend.signals.lock().unwrap().is_empty());
}

#[test]
fn conflicting_tag_is_sticky_and_keeps_original_owner() {
    let (_dir, engine, s) = setup();
    record(&engine, &s, 1_900_000);
    let other = engine
        .begin_session("other", "/same/project", None)
        .unwrap();
    engine
        .backend
        .mutate(1_900_000, |p| p.session_tag = Some(other.id));
    engine.observe().unwrap();
    engine
        .backend
        .mutate(1_900_000, |p| p.session_tag = Some(s.id.clone()));
    engine.observe().unwrap();
    engine.end_session(&s.id, "done").unwrap();
    let resource = engine.plan_cleanup(&s.id).unwrap().resources.remove(0);
    assert!(!resource.cleanup_eligible);
    assert!(resource.reasons.iter().any(|r| r.contains("conflicting")));
}

#[test]
fn observed_manager_ownership_stays_report_only_for_detached_descendants() {
    let (_dir, engine, s) = setup();
    let parent = record(&engine, &s, 1_910_001);
    let mut child = process(1_910_000, engine.backend.uid, Some(&s.id));
    child.parent_pid = parent.identity.pid;
    engine.backend.put(child);
    engine.register_owned(&s.id, 1_910_000).unwrap();
    engine
        .backend
        .mutate(parent.identity.pid, |p| p.manager_owned = Some(true));
    engine.observe().unwrap();
    engine.backend.remove(parent.identity.pid);
    engine.backend.mutate(1_910_000, |p| p.parent_pid = 1);
    engine.observe().unwrap();
    engine.end_session(&s.id, "done").unwrap();
    let plan = engine.plan_cleanup(&s.id).unwrap();
    assert!(plan.resources.iter().all(|r| !r.cleanup_eligible));
    let child = plan
        .resources
        .iter()
        .find(|r| r.identity.pid == 1_910_000)
        .unwrap();
    assert!(child.reasons.iter().any(|r| r.contains("service-manager")));
}

#[test]
fn invoking_process_and_ancestry_are_protected_even_when_registered() {
    let (_dir, engine, s) = setup();
    let parent = record(&engine, &s, 1_920_000);
    engine.backend.mutate(std::process::id(), |p| {
        p.parent_pid = parent.identity.pid;
        p.session_tag = Some(s.id.clone());
    });
    engine.register_owned(&s.id, std::process::id()).unwrap();
    engine.end_session(&s.id, "done").unwrap();
    let plan = engine.plan_cleanup(&s.id).unwrap();
    assert_eq!(plan.resources.len(), 2);
    assert!(plan.resources.iter().all(|r| !r.cleanup_eligible));
    assert!(
        plan.resources
            .iter()
            .all(|r| r.reasons.iter().any(|reason| reason.contains("protected")))
    );
}

#[test]
fn concurrent_writers_and_empty_read_commands_are_recoverable() {
    let dir = TempDir::new();
    let reader = Store::open(&dir.0, false).unwrap();
    assert!(reader.sessions().unwrap().is_empty());
    assert!(!dir.0.exists());
    drop(reader);
    drop(Store::open(&dir.0, true).unwrap());
    let mut workers = Vec::new();
    for _ in 0..8 {
        let path = dir.0.clone();
        workers.push(std::thread::spawn(move || {
            let engine = Engine::new(Store::open(&path, true).unwrap(), FakeBackend::new());
            for _ in 0..10 {
                engine
                    .begin_session("concurrent", "/project", None)
                    .unwrap();
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        Store::open(&dir.0, false)
            .unwrap()
            .sessions()
            .unwrap()
            .len(),
        80
    );
}

#[test]
fn stale_observer_cannot_erase_conflict_manager_or_keep_lineage() {
    let (dir, engine, s) = setup();
    let parent = record(&engine, &s, 1_930_000);
    let child = record(&engine, &s, 1_930_001);
    let mut stale = engine
        .store
        .records()
        .unwrap()
        .into_iter()
        .find(|r| r.identity == child.identity)
        .unwrap();
    let mut newer = stale.clone();
    newer.conflict = true;
    newer.manager_owned = true;
    newer.parent_key = Some(parent.identity.key());
    newer.evidence.push(Evidence::ObservedAncestry);
    engine.store.save_record(&newer).unwrap();
    engine.set_keep(parent.identity.pid, true).unwrap();
    // Another observer held this old record before the newer write committed.
    stale.last_seen_ms = now_ms() + 1;
    let second = Store::open(&dir.0, true).unwrap();
    second.save_record(&stale).unwrap();
    let saved = engine
        .store
        .records()
        .unwrap()
        .into_iter()
        .find(|r| r.identity == child.identity)
        .unwrap();
    assert!(saved.conflict && saved.manager_owned);
    assert_eq!(saved.parent_key, Some(parent.identity.key()));
    assert!(saved.evidence.contains(&Evidence::ObservedAncestry));
    engine.end_session(&s.id, "done").unwrap();
    assert!(
        engine
            .plan_cleanup(&s.id)
            .unwrap()
            .resources
            .iter()
            .all(|r| !r.cleanup_eligible)
    );
    let other = engine.begin_session("other", "/test", None).unwrap();
    stale.session_id = other.id;
    second.save_record(&stale).unwrap();
    let saved = engine
        .store
        .records()
        .unwrap()
        .into_iter()
        .find(|r| r.identity == child.identity)
        .unwrap();
    assert_eq!(saved.session_id, s.id);
    assert!(saved.conflict);
}

#[test]
fn lsof_parser_handles_ipv6_duplicate_fds_and_multiple_listeners() {
    let ports = crate::platform::parse_lsof(
        "p123\nf4\nn127.0.0.1:3000\nn127.0.0.1:3000\np456\nf8\nn[::1]:3000\nn*:8080\n",
    )
    .unwrap();
    assert_eq!(ports.len(), 3);
    assert_eq!(ports[1].address, "::1");
}

#[test]
fn scan_200_recorded_processes_latency() {
    let (_dir, engine, s) = setup();
    let mut records = Vec::new();
    for i in 0..200 {
        let p = process(2_000_000 + i, engine.backend.uid, Some(&s.id));
        engine.backend.put(p.clone());
        records.push(OwnershipRecord {
            identity: p.identity,
            session_id: s.id.clone(),
            parent_key: None,
            executable: p.executable,
            name: p.name,
            evidence: vec![Evidence::InheritedTag],
            first_seen_ms: now_ms(),
            last_seen_ms: now_ms(),
            conflict: false,
            manager_owned: false,
        });
    }
    engine.store.save_records(&records).unwrap();
    engine.end_session(&s.id, "done").unwrap();
    let start = Instant::now();
    assert_eq!(engine.scan(None).unwrap().resources.len(), 200);
    assert!(start.elapsed() < Duration::from_secs(1));
}
