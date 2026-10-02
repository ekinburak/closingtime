use crate::{Backend, Result, Store, model::*, new_id, now_ms};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::{Child, Command},
    time::{Duration, Instant},
};

/// An explicit acknowledgment by an embedding harness that the plan was reviewed.
/// This is not a security token. A harness must implement its own human approval UI.
pub struct ReviewedApproval {
    session_id: String,
}
impl ReviewedApproval {
    pub fn for_plan(plan: &CleanupPlan) -> Self {
        Self {
            session_id: plan.session_id.clone(),
        }
    }
}

pub struct Engine<B: Backend> {
    pub store: Store,
    pub backend: B,
}
impl<B: Backend> Engine<B> {
    pub fn new(store: Store, backend: B) -> Self {
        Self { store, backend }
    }
    pub fn begin_session(
        &self,
        label: &str,
        project: &str,
        native_session_id: Option<String>,
    ) -> Result<Session> {
        let supervisor = match self.backend.inspect(std::process::id()) {
            Inspection::Present(p) => p.identity,
            _ => return Err("cannot identify this supervisor; no command was launched".into()),
        };
        let session = Session {
            id: new_id()?,
            label: label.into(),
            command: None,
            project: project.into(),
            native_session_id,
            host: self.backend.host().into(),
            boot: self.backend.boot().into(),
            uid: self.backend.uid(),
            supervisor,
            root: None,
            state: SessionState::Active,
            started_ms: now_ms(),
            ended_ms: None,
            outcome: None,
        };
        self.store.save_session(&session)?;
        Ok(session)
    }
    pub fn spawn_owned(&self, session_id: &str, command: &mut Command) -> Result<Child> {
        let mut session = self.store.session(session_id)?;
        self.validate_session(&session)?;
        if session.state != SessionState::Active {
            return Err("cannot spawn into an ended session".into());
        }
        if session.root.is_none() {
            session.command = Some(command.get_program().to_string_lossy().into_owned());
            self.store.save_session(&session)?;
        }
        command
            .env(SESSION_ENV, &session.id)
            .env(PROJECT_ENV, &session.project);
        let mut child = command.spawn()?;
        let registration = (|| -> Result<()> {
            match self.backend.inspect(child.id()) {
                Inspection::Present(p) => {
                    if session.root.is_none() {
                        session.root = Some(p.identity.clone());
                        self.store.save_session(&session)?;
                    }
                    self.register_process(&session.id, &p, Evidence::LaunchRegistration)?;
                }
                Inspection::Gone => {} // A fast command may have already exited; never invent identity.
                Inspection::Unavailable(e) => {
                    return Err(format!("launched process cannot be recorded: {e}").into());
                }
            }
            Ok(())
        })();
        if let Err(error) = registration {
            // Our unreaped direct child cannot have its PID recycled. Abort an unrecorded launch.
            let _ = child.kill();
            let _ = child.wait();
            session.state = SessionState::Ended;
            session.ended_ms = Some(now_ms());
            session.outcome = Some("registration_failed".into());
            let _ = self.store.save_session(&session);
            return Err(error);
        }
        Ok(child)
    }
    pub fn register_owned(&self, session_id: &str, pid: u32) -> Result<OwnershipRecord> {
        let session = self.store.session(session_id)?;
        self.validate_session(&session)?;
        if session.state != SessionState::Active {
            return Err("cannot register into an ended session".into());
        }
        match self.backend.inspect(pid) {
            Inspection::Present(p) => {
                self.register_process(session_id, &p, Evidence::ExplicitRegistration)
            }
            _ => Err("process is absent or unreadable".into()),
        }
    }
    fn validate_session(&self, session: &Session) -> Result<()> {
        if session.uid != self.backend.uid()
            || session.host != self.backend.host()
            || session.boot != self.backend.boot()
        {
            return Err("session belongs to another user, host, or boot".into());
        }
        Ok(())
    }
    fn register_process(
        &self,
        session_id: &str,
        p: &Process,
        evidence: Evidence,
    ) -> Result<OwnershipRecord> {
        let session = self.store.session(session_id)?;
        self.validate_session(&session)?;
        if p.identity.uid != session.uid
            || p.identity.host != session.host
            || p.identity.boot != session.boot
        {
            return Err("cannot register another user's process or another boot".into());
        }
        if p.session_tag
            .as_deref()
            .is_some_and(|tag| tag != session_id)
        {
            return Err("conflicting session tag".into());
        }
        let records = self.store.records()?;
        if records
            .iter()
            .any(|r| r.identity == p.identity && r.session_id != session_id)
        {
            return Err("process already belongs to a different session".into());
        }
        let old = records.iter().find(|r| r.identity == p.identity);
        let parent_key = match self.backend.inspect(p.parent_pid) {
            Inspection::Present(parent) => records
                .iter()
                .find(|r| r.identity == parent.identity && r.session_id == session_id)
                .map(|r| r.identity.key()),
            _ => None,
        };
        let mut record = old.cloned().unwrap_or(OwnershipRecord {
            identity: p.identity.clone(),
            session_id: session_id.into(),
            parent_key,
            executable: p.executable.clone(),
            name: p.name.clone(),
            evidence: Vec::new(),
            first_seen_ms: now_ms(),
            last_seen_ms: now_ms(),
            conflict: false,
            manager_owned: p.manager_owned == Some(true),
        });
        if !record.evidence.contains(&evidence) {
            record.evidence.push(evidence);
        }
        record.last_seen_ms = now_ms();
        record.executable = p.executable.clone();
        record.name = p.name.clone();
        record.manager_owned |= p.manager_owned == Some(true);
        self.store.save_record(&record)?;
        let saved = self
            .store
            .records()?
            .into_iter()
            .find(|r| r.identity == p.identity)
            .ok_or("registered identity was not persisted")?;
        if saved.session_id != session_id {
            return Err(
                "concurrent registration disagrees on ownership; cleanup is blocked".into(),
            );
        }
        Ok(saved)
    }
    /// Call while supervising a run and once after wait(). Observation is not spawn interception.
    pub fn observe(&self) -> Result<usize> {
        let snapshot = self.backend.snapshot()?;
        let sessions: BTreeMap<_, _> = self
            .store
            .sessions()?
            .into_iter()
            .filter(|s| self.validate_session(s).is_ok())
            .map(|s| (s.id.clone(), s))
            .collect();
        let mut records: BTreeMap<_, _> = self
            .store
            .records()?
            .into_iter()
            .map(|r| (r.identity.key(), r))
            .collect();
        let mut changed = BTreeMap::new();
        // Iterate to connect a lineage even when child PIDs sort before their parents.
        for _ in 0..snapshot.processes.len().max(1) {
            let mut additions = 0;
            let mut manager_changes = 0;
            for p in snapshot
                .processes
                .values()
                .filter(|p| p.identity.uid == self.backend.uid())
            {
                let key = p.identity.key();
                let parent = snapshot
                    .processes
                    .get(&p.parent_pid)
                    .and_then(|parent| records.get(&parent.identity.key()))
                    .filter(|r| !r.conflict);
                let tag = p
                    .session_tag
                    .as_ref()
                    .filter(|tag| sessions.contains_key(*tag));
                let inferred = parent.map(|r| &r.session_id);
                let existing = records.get(&key);
                let owner = existing
                    .map(|r| &r.session_id)
                    .or(tag)
                    .or(inferred)
                    .cloned();
                let Some(owner) = owner else {
                    continue;
                };
                if !sessions.contains_key(&owner) {
                    continue;
                }
                let conflict = existing.is_some_and(|r| r.conflict)
                    || p.session_tag.as_ref().is_some_and(|tag| tag != &owner)
                    || inferred.is_some_and(|id| id != &owner);
                let first = existing.is_none();
                let mut record = existing.cloned().unwrap_or(OwnershipRecord {
                    identity: p.identity.clone(),
                    session_id: owner.clone(),
                    parent_key: parent
                        .filter(|r| r.session_id == owner)
                        .map(|r| r.identity.key()),
                    executable: p.executable.clone(),
                    name: p.name.clone(),
                    evidence: Vec::new(),
                    first_seen_ms: now_ms(),
                    last_seen_ms: now_ms(),
                    conflict,
                    manager_owned: false,
                });
                for evidence in [
                    tag.map(|_| Evidence::InheritedTag),
                    inferred
                        .filter(|id| *id == &owner)
                        .map(|_| Evidence::ObservedAncestry),
                ]
                .into_iter()
                .flatten()
                {
                    if !record.evidence.contains(&evidence) {
                        record.evidence.push(evidence);
                    }
                }
                record.conflict = conflict;
                record.manager_owned |=
                    p.manager_owned == Some(true) || parent.is_some_and(|r| r.manager_owned);
                if record.manager_owned && existing.is_none_or(|r| !r.manager_owned) {
                    manager_changes += 1;
                }
                record.last_seen_ms = now_ms();
                record.executable = p.executable.clone();
                record.name = p.name.clone();
                if record.parent_key.is_none() {
                    record.parent_key = parent
                        .filter(|r| r.session_id == owner && r.identity != p.identity)
                        .map(|r| r.identity.key());
                }
                let different = existing.is_none_or(|old| {
                    old.executable != record.executable
                        || old.evidence != record.evidence
                        || old.conflict != record.conflict
                        || old.manager_owned != record.manager_owned
                        || old.parent_key != record.parent_key
                        || now_ms().saturating_sub(old.last_seen_ms) >= 5000
                });
                if different {
                    changed.insert(key.clone(), record.clone());
                }
                records.insert(key, record);
                if first {
                    additions += 1;
                }
            }
            if additions == 0 && manager_changes == 0 {
                break;
            }
        }
        let changes = changed.len();
        if changes > 0 {
            self.store
                .save_records(&changed.into_values().collect::<Vec<_>>())?;
        }
        Ok(changes)
    }
    pub fn end_session(&self, session_id: &str, outcome: &str) -> Result<()> {
        let mut session = self.store.session(session_id)?;
        self.validate_session(&session)?;
        if let Some(root) = &session.root {
            match self.backend.inspect(root.pid) {
                Inspection::Gone => {}
                Inspection::Present(p) if p.identity != *root || p.zombie => {}
                _ => return Err("root is still running or its exit cannot be verified".into()),
            }
        }
        session.state = SessionState::Ended;
        session.ended_ms = Some(now_ms());
        session.outcome = Some(outcome.into());
        self.store.save_session(&session)
    }
    pub fn set_keep(&self, pid: u32, keep: bool) -> Result<()> {
        let p = match self.backend.inspect(pid) {
            Inspection::Present(p) => p,
            _ => return Err("process is absent or unreadable".into()),
        };
        if !self
            .store
            .records()?
            .iter()
            .any(|r| r.identity == p.identity && r.identity.uid == self.backend.uid())
        {
            return Err("process is not recorded for this user".into());
        }
        self.store.set_keep(&p.identity.key(), keep)
    }
    pub fn scan(&self, session_id: Option<&str>) -> Result<Scan> {
        if let Some(id) = session_id {
            self.store.session(id)?;
        }
        let snapshot = self.backend.snapshot()?;
        self.scan_snapshot(&snapshot, session_id)
    }
    fn scan_snapshot(&self, snapshot: &Snapshot, session_id: Option<&str>) -> Result<Scan> {
        let sessions: BTreeMap<_, _> = self
            .store
            .sessions()?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        let records = self.store.records()?;
        let kept = inherited_keeps(&records, self.store.kept()?);
        let protected = protected_pids(snapshot);
        let mut resources = Vec::new();
        for record in records
            .iter()
            .filter(|r| session_id.is_none_or(|id| r.session_id == id))
        {
            let session = sessions.get(&record.session_id);
            let mut resource = view(record, session, snapshot, &kept, &protected, &self.backend);
            // A detached child observed after its parent vanished may be a kept descendant.
            // If that lineage is incomplete, preserve it whenever this run has a keep.
            let has_keep = records
                .iter()
                .any(|r| r.session_id == record.session_id && kept.contains(&r.identity.key()));
            let is_root = session.and_then(|s| s.root.as_ref()) == Some(&record.identity);
            if has_keep && record.parent_key.is_none() && !is_root && !resource.kept {
                resource
                    .reasons
                    .push("keep ancestry is incomplete; report only".into());
                resource.cleanup_eligible = false;
            }
            resources.push(resource);
        }
        // Newly found tags are useful to report, but read commands must not silently register them.
        for p in snapshot
            .processes
            .values()
            .filter(|p| p.identity.uid == self.backend.uid())
        {
            if records.iter().any(|r| r.identity == p.identity) {
                continue;
            }
            if let Some(id) = p.session_tag.as_deref().filter(|id| {
                sessions.contains_key(*id) && session_id.is_none_or(|filter| filter == *id)
            }) {
                resources.push(ResourceView {
                    identity: p.identity.clone(),
                    session_id: Some(id.into()),
                    session_label: sessions.get(id).map(|s| s.label.clone()),
                    project: sessions.get(id).map(|s| s.project.clone()),
                    command: sessions.get(id).and_then(|s| s.command.clone()),
                    name: p.name.clone(),
                    executable: p.executable.clone(),
                    evidence: vec![Evidence::InheritedTag],
                    ports: ports_for(p, snapshot),
                    status: "unrecorded".into(),
                    kept: false,
                    cleanup_eligible: false,
                    reasons: vec!["tagged process was not recorded by the supervisor".into()],
                });
            }
        }
        Ok(Scan {
            schema: SCHEMA.into(),
            resources,
            warnings: snapshot.warnings.clone(),
        })
    }
    pub fn who_pid(&self, pid: u32) -> Result<Scan> {
        let snapshot = self.backend.snapshot()?;
        let mut scan = self.scan_snapshot(&snapshot, None)?;
        scan.resources.retain(|r| {
            r.identity.pid == pid
                && r.identity.host == self.backend.host()
                && r.identity.boot == self.backend.boot()
                && r.status != "gone"
                && r.status != "identity_changed"
        });
        if scan.resources.is_empty() {
            if let Some(p) = snapshot.processes.get(&pid) {
                scan.resources.push(ResourceView {
                    identity: p.identity.clone(),
                    session_id: None,
                    session_label: None,
                    project: None,
                    command: None,
                    name: p.name.clone(),
                    executable: p.executable.clone(),
                    evidence: Vec::new(),
                    ports: ports_for(p, &snapshot),
                    status: "unknown".into(),
                    kept: false,
                    cleanup_eligible: false,
                    reasons: vec!["no recorded ownership".into()],
                });
            } else {
                scan.warnings
                    .push(format!("PID {pid} is absent or unreadable"));
            }
        }
        Ok(scan)
    }
    pub fn who_port(&self, port: u16) -> Result<Scan> {
        let snapshot = self.backend.snapshot()?;
        let mut scan = self.scan_snapshot(&snapshot, None)?;
        scan.resources
            .retain(|r| r.ports.iter().any(|p| p.port == port));
        for p in snapshot.ports.iter().filter(|p| p.port == port) {
            if scan.resources.iter().any(|r| r.identity.pid == p.pid) {
                continue;
            }
            if let Some(process) = snapshot.processes.get(&p.pid) {
                scan.resources.push(ResourceView {
                    identity: process.identity.clone(),
                    session_id: None,
                    session_label: None,
                    project: None,
                    command: None,
                    name: process.name.clone(),
                    executable: process.executable.clone(),
                    evidence: Vec::new(),
                    ports: ports_for(process, &snapshot),
                    status: "unknown".into(),
                    kept: false,
                    cleanup_eligible: false,
                    reasons: vec!["no recorded ownership".into()],
                });
            } else {
                scan.warnings
                    .push(format!("port {port} is held by unreadable PID {}", p.pid));
            }
        }
        Ok(scan)
    }
    pub fn plan_cleanup(&self, session_id: &str) -> Result<CleanupPlan> {
        let scan = self.scan(Some(session_id))?;
        Ok(CleanupPlan {
            schema: SCHEMA.into(),
            session_id: session_id.into(),
            created_ms: now_ms(),
            resources: scan.resources,
            warnings: scan.warnings,
        })
    }
    fn guarded_signal(
        &self,
        plan: &CleanupPlan,
        reviewed: &ResourceView,
        signal: i32,
    ) -> Result<bool> {
        // Serialize with keep/session writers during the final check and signal.
        self.store.connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<bool> {
            let fresh = self.plan_cleanup(&plan.session_id)?;
            let matches = fresh.resources.iter().any(|r| {
                r.cleanup_eligible
                    && r.identity == reviewed.identity
                    && r.executable == reviewed.executable
            });
            if !matches {
                return Ok(false);
            }
            self.backend.signal(&reviewed.identity, signal)?;
            Ok(true)
        })();
        match result {
            Ok(sent) => {
                self.store.connection.execute_batch("COMMIT")?;
                Ok(sent)
            }
            Err(e) => {
                let _ = self.store.connection.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
    pub fn apply_plan(
        &self,
        plan: &CleanupPlan,
        approval: ReviewedApproval,
    ) -> Result<Vec<Action>> {
        if plan.schema != SCHEMA || approval.session_id != plan.session_id {
            return Err("approval/schema does not match the plan".into());
        }
        let mut actions = Vec::new();
        for resource in plan.resources.iter().filter(|r| r.cleanup_eligible) {
            let mut action = Action {
                id: new_id()?,
                session_id: plan.session_id.clone(),
                identity: resource.identity.clone(),
                actor_uid: self.backend.uid(),
                started_ms: now_ms(),
                finished_ms: None,
                result: "pending".into(),
                signals: Vec::new(),
                evidence: resource.evidence.clone(),
            };
            // Durable intent first. If the ledger is broken, do not signal.
            self.store.save_action(&action)?;
            match self.guarded_signal(plan, resource, libc::SIGTERM) {
                Ok(false) => action.result = "skipped_after_recheck".into(),
                Err(e) => action.result = format!("refused: {e}"),
                Ok(true) => {
                    action.signals.push("SIGTERM".into());
                    self.store.save_action(&action)?;
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while Instant::now() < deadline
                        && still_running(&self.backend, &resource.identity)
                    {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    if still_running(&self.backend, &resource.identity) {
                        match self.guarded_signal(plan, resource, libc::SIGKILL) {
                            Ok(true) => {
                                action.signals.push("SIGKILL".into());
                            }
                            Ok(false) => action.result = "escalation_skipped_after_recheck".into(),
                            Err(e) => action.result = format!("escalation_refused: {e}"),
                        }
                        let deadline = Instant::now() + Duration::from_secs(1);
                        while Instant::now() < deadline
                            && still_running(&self.backend, &resource.identity)
                        {
                            std::thread::sleep(Duration::from_millis(25));
                        }
                    }
                    if action.result == "pending" {
                        action.result = if still_running(&self.backend, &resource.identity) {
                            "still_running"
                        } else {
                            "stopped_or_awaiting_reaping"
                        }
                        .into();
                    }
                }
            }
            action.finished_ms = Some(now_ms());
            self.store.save_action(&action)?;
            actions.push(action);
        }
        Ok(actions)
    }
}

fn still_running<B: Backend>(backend: &B, identity: &ProcessIdentity) -> bool {
    match backend.inspect(identity.pid) {
        Inspection::Present(p) => p.identity == *identity && !p.zombie,
        Inspection::Gone => false,
        Inspection::Unavailable(_) => true, // Unknown must not be reported as successfully stopped.
    }
}
fn inherited_keeps(records: &[OwnershipRecord], mut kept: BTreeSet<String>) -> BTreeSet<String> {
    loop {
        let before = kept.len();
        for r in records {
            if r.parent_key.as_ref().is_some_and(|p| kept.contains(p)) {
                kept.insert(r.identity.key());
            }
        }
        if kept.len() == before {
            return kept;
        }
    }
}
fn protected_pids(snapshot: &Snapshot) -> BTreeSet<u32> {
    let mut protected = BTreeSet::from([0, 1, std::process::id()]);
    let mut pid = std::process::id();
    while let Some(p) = snapshot.processes.get(&pid) {
        pid = p.parent_pid;
        if !protected.insert(pid) {
            break;
        }
    }
    for p in snapshot.processes.values() {
        if matches!(
            p.name.as_str(),
            "closingtime"
                | "launchd"
                | "systemd"
                | "kernel_task"
                | "WindowServer"
                | "Cursor"
                | "Code"
                | "Terminal"
                | "iTerm2"
        ) {
            protected.insert(p.identity.pid);
        }
    }
    protected
}
fn ports_for(p: &Process, snapshot: &Snapshot) -> Vec<Port> {
    snapshot
        .ports
        .iter()
        .filter(|port| port.pid == p.identity.pid)
        .cloned()
        .collect()
}
fn view<B: Backend>(
    record: &OwnershipRecord,
    session: Option<&Session>,
    snapshot: &Snapshot,
    kept: &BTreeSet<String>,
    protected: &BTreeSet<u32>,
    backend: &B,
) -> ResourceView {
    let p = snapshot
        .processes
        .get(&record.identity.pid)
        .filter(|p| p.identity == record.identity);
    let mut reasons = Vec::new();
    if record.identity.uid != backend.uid()
        || record.identity.host != backend.host()
        || record.identity.boot != backend.boot()
    {
        reasons.push("different user, host, or boot".into());
    }
    if record.conflict {
        reasons.push("conflicting ownership evidence".into());
    }
    if record.manager_owned {
        reasons.push("recorded service-manager ownership; report only".into());
    }
    let kept = kept.contains(&record.identity.key());
    if kept {
        reasons.push("kept process or descendant".into());
    }
    if protected.contains(&record.identity.pid) {
        reasons.push("protected process or invoking ancestor".into());
    }
    match session {
        Some(s) if s.state == SessionState::Ended && s.ended_ms.is_some() => {
            if let Some(root) = &s.root {
                if snapshot.unavailable.contains_key(&root.pid) {
                    reasons.push("root exit cannot be verified".into());
                }
                if snapshot
                    .processes
                    .get(&root.pid)
                    .is_some_and(|p| p.identity == *root && !p.zombie)
                {
                    reasons.push("session root still running".into());
                }
            }
        }
        _ => reasons.push("session end is not confirmed".into()),
    }
    let status = if let Some(p) = p {
        if p.zombie {
            reasons.push("zombie awaiting parent reaping".into());
        }
        if p.executable.is_none() || p.executable != record.executable {
            reasons.push("executable identity unavailable or changed".into());
        }
        if !p.tag_readable {
            reasons.push("current ownership metadata unreadable".into());
        }
        if p.session_tag
            .as_deref()
            .is_some_and(|tag| tag != record.session_id)
        {
            reasons.push("current tag conflicts with recorded owner".into());
        }
        if p.manager_owned != Some(false) {
            reasons.push("manager-owned service or manager status unknown".into());
        }
        if p.zombie { "zombie" } else { "running" }
    } else if snapshot.unavailable.contains_key(&record.identity.pid) {
        reasons.push("process metadata unreadable".into());
        "unreadable"
    } else if snapshot.processes.contains_key(&record.identity.pid) {
        reasons.push("PID identity changed".into());
        "identity_changed"
    } else {
        reasons.push("process is gone".into());
        "gone"
    };
    ResourceView {
        identity: record.identity.clone(),
        session_id: Some(record.session_id.clone()),
        session_label: session.map(|s| s.label.clone()),
        project: session.map(|s| s.project.clone()),
        command: session.and_then(|s| s.command.clone()),
        name: record.name.clone(),
        executable: record.executable.clone(),
        evidence: record.evidence.clone(),
        ports: p.map(|p| ports_for(p, snapshot)).unwrap_or_default(),
        status: status.into(),
        kept,
        cleanup_eligible: reasons.is_empty(),
        reasons,
    }
}

#[cfg(test)]
mod tests;
