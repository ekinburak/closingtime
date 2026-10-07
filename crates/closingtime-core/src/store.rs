use crate::{Result, model::*, now_ms};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Store {
    pub(crate) connection: Connection,
    writable: bool,
}

pub fn default_state_dir() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("CLOSINGTIME_STATE_DIR") {
        return Ok(p.into());
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set; use --state-dir")?;
    if cfg!(target_os = "macos") {
        Ok(PathBuf::from(home).join("Library/Application Support/closingtime"))
    } else if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        Ok(PathBuf::from(p).join("closingtime"))
    } else {
        Ok(PathBuf::from(home).join(".local/state/closingtime"))
    }
}

impl Store {
    pub fn open(dir: &Path, writable: bool) -> Result<Self> {
        let path = dir.join("ledger.db");
        if writable {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
            let metadata = fs::symlink_metadata(dir)?;
            if metadata.file_type().is_symlink() {
                return Err("state directory must not be a symlink".into());
            }
            if metadata.uid() != unsafe { libc::geteuid() } {
                return Err("state directory belongs to another user".into());
            }
            if metadata.mode() & 0o077 != 0 {
                return Err(
                    "state directory must be private (mode 0700); choose a dedicated directory"
                        .into(),
                );
            }
            if !path.exists() {
                let _file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path);
            }
        }
        if path.exists() {
            let m = fs::symlink_metadata(&path)?;
            if !m.is_file()
                || m.file_type().is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o077 != 0
            {
                return Err("ledger must be a regular file owned by the current user".into());
            }
        }
        if !writable && !path.exists() {
            let connection = Connection::open_in_memory()?;
            connection.execute_batch("CREATE TABLE sessions (id TEXT PRIMARY KEY, data TEXT NOT NULL);
                CREATE TABLE processes (identity TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE keeps (identity TEXT PRIMARY KEY); CREATE TABLE actions (id TEXT PRIMARY KEY, data TEXT NOT NULL);")?;
            return Ok(Self {
                connection,
                writable: false,
            });
        }
        let flags = if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let connection = Connection::open_with_flags(path, flags)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        if writable {
            connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
                CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS processes (identity TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS keeps (identity TEXT PRIMARY KEY);
                CREATE TABLE IF NOT EXISTS actions (id TEXT PRIMARY KEY, data TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY, at_ms INTEGER NOT NULL, kind TEXT NOT NULL, data TEXT NOT NULL);")?;
        }
        Ok(Self {
            connection,
            writable,
        })
    }
    fn require_write(&self) -> Result<()> {
        if !self.writable {
            Err("ledger is read-only".into())
        } else {
            Ok(())
        }
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        let data: String =
            self.connection
                .query_row("SELECT data FROM sessions WHERE id=?", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&data)?)
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        self.read_rows("SELECT data FROM sessions ORDER BY id")
    }
    pub fn records(&self) -> Result<Vec<OwnershipRecord>> {
        self.read_rows("SELECT data FROM processes ORDER BY identity")
    }
    pub fn actions(&self) -> Result<Vec<Action>> {
        self.read_rows("SELECT data FROM actions ORDER BY id")
    }
    fn read_rows<T: serde::de::DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>> {
        let mut statement = self.connection.prepare(sql)?;
        let data = statement.query_map([], |r| r.get::<_, String>(0))?;
        data.map(|d| Ok(serde_json::from_str(&d?)?)).collect()
    }
    pub fn kept(&self) -> Result<std::collections::BTreeSet<String>> {
        let mut s = self.connection.prepare("SELECT identity FROM keeps")?;
        Ok(s.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn save_session(&self, session: &Session) -> Result<()> {
        self.require_write()?;
        let data = serde_json::to_string(session)?;
        let tx = self.connection.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO sessions VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![session.id, data],
        )?;
        tx.execute(
            "INSERT INTO events(at_ms,kind,data) VALUES (?,?,?)",
            params![now_ms() as i64, "session", data],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn save_record(&self, record: &OwnershipRecord) -> Result<()> {
        self.save_records(std::slice::from_ref(record))
    }
    pub(crate) fn save_records(&self, records: &[OwnershipRecord]) -> Result<()> {
        self.require_write()?;
        // Observers may have read before another writer recorded a conflict/keep
        // lineage. Merge under the write lock; stale observations cannot erase it.
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        for record in records {
            let old: Option<String> = tx
                .query_row(
                    "SELECT data FROM processes WHERE identity=?",
                    [record.identity.key()],
                    |row| row.get(0),
                )
                .optional()?;
            let mut merged = record.clone();
            if let Some(old) = old {
                let old: OwnershipRecord = serde_json::from_str(&old)?;
                if old.session_id != merged.session_id {
                    // Never move an existing identity between owners. Preserve the
                    // first owner and record the disagreement as a durable conflict.
                    merged = old;
                    merged.conflict = true;
                } else {
                    merged.conflict |= old.conflict;
                    merged.manager_owned |= old.manager_owned;
                    merged.parent_key = old.parent_key.or(merged.parent_key);
                    merged.first_seen_ms = old.first_seen_ms.min(merged.first_seen_ms);
                    if old.last_seen_ms > merged.last_seen_ms {
                        merged.executable = old.executable;
                        merged.name = old.name;
                    }
                    merged.last_seen_ms = old.last_seen_ms.max(merged.last_seen_ms);
                    for evidence in old.evidence {
                        if !merged.evidence.contains(&evidence) {
                            merged.evidence.push(evidence);
                        }
                    }
                }
            }
            let data = serde_json::to_string(&merged)?;
            tx.execute("INSERT INTO processes VALUES (?,?,?) ON CONFLICT(identity) DO UPDATE SET data=excluded.data", params![merged.identity.key(), merged.session_id, data])?;
            tx.execute(
                "INSERT INTO events(at_ms,kind,data) VALUES (?,?,?)",
                params![now_ms() as i64, "observation", data],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn set_keep(&self, key: &str, keep: bool) -> Result<()> {
        self.require_write()?;
        let tx = self.connection.unchecked_transaction()?;
        if keep {
            tx.execute("INSERT OR IGNORE INTO keeps VALUES (?)", [key])?;
        } else {
            tx.execute("DELETE FROM keeps WHERE identity=?", [key])?;
        }
        tx.execute(
            "INSERT INTO events(at_ms,kind,data) VALUES (?,?,?)",
            params![now_ms() as i64, if keep { "keep" } else { "unkeep" }, key],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn save_action(&self, action: &Action) -> Result<()> {
        self.require_write()?;
        self.connection.execute(
            "INSERT INTO actions VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            params![action.id, serde_json::to_string(action)?],
        )?;
        Ok(())
    }
    pub(crate) fn prune(&self, host: &str, boot: &str, events_before_ms: u64) -> Result<Pruned> {
        self.require_write()?;
        let elsewhere = |identity: &ProcessIdentity| identity.host != host || identity.boot != boot;
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let mut pruned = Pruned::default();
        for session in self.sessions()? {
            if session.host != host || session.boot != boot {
                pruned.sessions += tx.execute("DELETE FROM sessions WHERE id=?", [&session.id])?;
            }
        }
        for record in self.records()? {
            if elsewhere(&record.identity) {
                pruned.processes += tx.execute(
                    "DELETE FROM processes WHERE identity=?",
                    [record.identity.key()],
                )?;
            }
        }
        for key in self.kept()? {
            // A key that no longer parses cannot match any live process either.
            if serde_json::from_str(&key).map_or(true, |id: ProcessIdentity| elsewhere(&id)) {
                pruned.keeps += tx.execute("DELETE FROM keeps WHERE identity=?", [&key])?;
            }
        }
        pruned.events = tx.execute(
            "DELETE FROM events WHERE at_ms < ?",
            [events_before_ms as i64],
        )?;
        tx.execute(
            "INSERT INTO events(at_ms,kind,data) VALUES (?,?,?)",
            params![now_ms() as i64, "prune", serde_json::to_string(&pruned)?],
        )?;
        tx.commit()?;
        Ok(pruned)
    }
    pub fn export(&self) -> Result<Export> {
        // A consistent snapshot across all tables, including concurrent writers.
        let tx = self.connection.unchecked_transaction()?;
        let result = Export {
            schema: SCHEMA.into(),
            sessions: self.sessions()?,
            processes: self.records()?,
            kept: self.kept()?,
            actions: self.actions()?,
        };
        tx.commit()?;
        Ok(result)
    }
}
