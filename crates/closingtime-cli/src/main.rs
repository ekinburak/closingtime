use clap::{Args, Parser, Subcommand};
use closingtime_core::*;
use serde::Serialize;
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "Record what a coding-agent run starts and review what survives"
)]
struct Cli {
    #[arg(long, global = true, help = "Private directory for the local ledger")]
    state_dir: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Machine-readable output (read commands and keep decisions)"
    )]
    json: bool,
    #[command(subcommand)]
    command: Subcommands,
}
#[derive(Subcommand)]
enum Subcommands {
    /// Launch a recorded run. Does not automatically clean up when it ends.
    Run {
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        native_session_id: Option<String>,
        #[arg(required = true, last = true)]
        command: Vec<String>,
    },
    /// List recorded runs and surviving processes.
    Sessions,
    /// Inspect resources belonging to recorded runs without changing the ledger.
    Scan(RunSelector),
    /// Explain a PID or a TCP listening port, including unknown ownership.
    Who(Who),
    /// Preview cleanup; --apply refreshes the plan and requires terminal approval.
    Clean {
        #[command(flatten)]
        run: RunSelector,
        #[arg(long)]
        apply: bool,
    },
    /// End a run whose supervisor crashed or stopped recording, once its root has exited.
    Recover(RunSelector),
    /// Forget runs from earlier boots and ledger events older than --event-days.
    Prune {
        #[arg(long, default_value_t = 30)]
        event_days: u64,
    },
    /// Preserve a recorded process and its known descendants.
    Keep {
        #[arg(long)]
        pid: u32,
    },
    /// Remove an explicit keep decision (ancestor keeps still apply).
    Unkeep {
        #[arg(long)]
        pid: u32,
    },
    /// Check platform identity, process metadata and collector capabilities.
    Doctor,
    /// Export the versioned ledger for another tool. Includes local action history.
    Export,
}
#[derive(Args)]
struct RunSelector {
    /// Run ID, or a unique prefix of one
    #[arg(long)]
    session: Option<String>,
    /// The most recently started run
    #[arg(long, conflicts_with = "session")]
    last: bool,
}
#[derive(Args)]
#[group(required = true, multiple = false)]
struct Who {
    #[arg(long)]
    pid: Option<u32>,
    #[arg(long, value_parser=clap::value_parser!(u16).range(1..))]
    port: Option<u16>,
}

fn main() {
    match execute(Cli::parse()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("closingtime: {}", safe(&error.to_string()));
            std::process::exit(5);
        }
    }
}
fn execute(cli: Cli) -> Result<i32> {
    let backend = NativeBackend::new()?;
    if matches!(cli.command, Subcommands::Doctor) {
        let doctor = backend.doctor()?;
        if cli.json {
            json(&doctor)?;
        } else {
            println!(
                "Platform: {}\nProcess identity: {}\nEnvironment metadata: {}\nTCP inventory: {}\nLinux pidfds: {}",
                doctor.platform,
                doctor.identity_available,
                doctor.tags_readable,
                doctor.ports_available,
                doctor.pidfd_available
            );
            warnings(&doctor.warnings);
        }
        return Ok(if doctor.identity_available && doctor.tags_readable {
            0
        } else {
            3
        });
    }
    let writable = matches!(
        cli.command,
        Subcommands::Run { .. }
            | Subcommands::Keep { .. }
            | Subcommands::Unkeep { .. }
            | Subcommands::Clean { apply: true, .. }
            | Subcommands::Recover(_)
            | Subcommands::Prune { .. }
    );
    let dir = cli.state_dir.unwrap_or(default_state_dir()?);
    if matches!(cli.command, Subcommands::Clean { apply: true, .. })
        && (!io::stdin().is_terminal() || !io::stdout().is_terminal() || cli.json)
    {
        return Err("--apply requires an interactive terminal and human output; noninteractive cleanup is refused".into());
    }
    if matches!(cli.command, Subcommands::Run { .. }) && cli.json {
        return Err("--json is for read commands; the child inherits normal terminal I/O".into());
    }
    let engine = Engine::new(Store::open(&dir, writable)?, backend);
    match cli.command {
        Subcommands::Run {
            label,
            native_session_id,
            command,
        } => run(&engine, label, native_session_id, command),
        Subcommands::Sessions => {
            let mut sessions = engine.store.sessions()?;
            sessions.sort_by_key(|s| s.started_ms);
            let scan = engine.scan(None)?;
            #[derive(Serialize)]
            struct Entry {
                session: Session,
                live_resources: usize,
                eligible_resources: usize,
            }
            let entries: Vec<_> = sessions
                .into_iter()
                .map(|s| {
                    let resources: Vec<_> = scan
                        .resources
                        .iter()
                        .filter(|r| r.session_id.as_deref() == Some(&s.id))
                        .collect();
                    Entry {
                        live_resources: resources
                            .iter()
                            .filter(|r| r.status == "running" || r.status == "unrecorded")
                            .count(),
                        eligible_resources: resources.iter().filter(|r| r.cleanup_eligible).count(),
                        session: s,
                    }
                })
                .collect();
            if cli.json {
                json(
                    &serde_json::json!({"schema":SCHEMA,"sessions":entries,"warnings":scan.warnings}),
                )?;
            } else {
                println!(
                    "RUN                               STATE   LIVE  ELIGIBLE  LABEL / PROJECT"
                );
                for e in entries {
                    println!(
                        "{}  {:?}  {:4}  {:8}  {} / {}",
                        e.session.id,
                        e.session.state,
                        e.live_resources,
                        e.eligible_resources,
                        safe(&e.session.label),
                        safe(&e.session.project)
                    );
                }
                warnings(&scan.warnings);
            }
            Ok(0)
        }
        Subcommands::Scan(selector) => {
            let session = select(&engine, &selector)?;
            let scan = engine.scan(session.as_deref())?;
            show_scan(&scan, cli.json)?;
            Ok(if scan.resources.iter().any(|r| r.cleanup_eligible) {
                1
            } else {
                0
            })
        }
        Subcommands::Who(who) => {
            let scan = if let Some(pid) = who.pid {
                engine.who_pid(pid)?
            } else {
                engine.who_port(who.port.unwrap())?
            };
            show_scan(&scan, cli.json)?;
            Ok(if scan.resources.is_empty() { 2 } else { 0 })
        }
        Subcommands::Clean { run, apply } => {
            let session = required(select(&engine, &run)?)?;
            let plan = engine.plan_cleanup(&session)?;
            if cli.json {
                json(&plan)?;
            } else {
                show_plan(&plan);
            }
            if !apply {
                return Ok(0);
            }
            let count = plan.resources.iter().filter(|r| r.cleanup_eligible).count();
            if count == 0 {
                println!("No eligible processes to stop.");
                return Ok(0);
            }
            print!(
                "Stop these {count} processes from run {}? Type yes: ",
                safe(&session)
            );
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().read_line(&mut answer)?;
            if answer.trim() != "yes" {
                println!("Cancelled.");
                return Ok(0);
            }
            let actions = engine.apply_plan(&plan, ReviewedApproval::for_plan(&plan))?;
            let failed = actions
                .iter()
                .any(|a| a.result != "stopped_or_awaiting_reaping");
            for action in actions {
                println!("PID {}: {}", action.identity.pid, safe(&action.result));
            }
            let remaining = engine.scan(Some(&session))?;
            let live = remaining
                .resources
                .iter()
                .filter(|r| r.status == "running" || r.status == "unrecorded")
                .count();
            println!("{live} surviving processes (including kept or ineligible items).");
            warnings(&remaining.warnings);
            Ok(if failed { 1 } else { 0 })
        }
        Subcommands::Keep { pid } | Subcommands::Unkeep { pid } => {
            let keep = matches!(cli.command, Subcommands::Keep { .. });
            engine.set_keep(pid, keep)?;
            if cli.json {
                json(&serde_json::json!({"schema":SCHEMA,"pid":pid,"kept":keep}))?;
            } else {
                println!(
                    "PID {pid}: {}",
                    if keep {
                        "kept with known descendants"
                    } else {
                        "explicit keep removed; ancestor keeps still apply"
                    }
                );
            }
            Ok(0)
        }
        Subcommands::Recover(selector) => {
            let session = required(select(&engine, &selector)?)?;
            engine.recover_session(&session)?;
            if cli.json {
                json(&serde_json::json!({"schema":SCHEMA,"session":session,"ended":true}))?;
            } else {
                println!(
                    "Run {} ended. Preview: closingtime clean --session {}",
                    safe(&session),
                    safe(&session)
                );
            }
            Ok(0)
        }
        Subcommands::Prune { event_days } => {
            let pruned = engine.prune(Duration::from_secs(event_days.saturating_mul(86_400)))?;
            if cli.json {
                json(&serde_json::json!({"schema":SCHEMA,"pruned":pruned}))?;
            } else {
                println!(
                    "Forgot {} runs, {} processes and {} keeps from earlier boots, and {} old events.",
                    pruned.sessions, pruned.processes, pruned.keeps, pruned.events
                );
            }
            Ok(0)
        }
        Subcommands::Export => {
            let export = engine.store.export()?;
            json(&export)?;
            Ok(0)
        }
        Subcommands::Doctor => unreachable!(),
    }
}
fn run(
    engine: &Engine<NativeBackend>,
    label: Option<String>,
    native: Option<String>,
    args: Vec<String>,
) -> Result<i32> {
    let project = std::env::current_dir()?.to_string_lossy().into_owned();
    let session = engine.begin_session(label.as_deref().unwrap_or(&args[0]), &project, native)?;
    let signal = Arc::new(AtomicUsize::new(0));
    let mut handlers = Vec::new();
    for value in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        if value == libc::SIGINT {
            let signal = signal.clone();
            // Linux distinguishes kernel terminal interrupts from explicitly sent SIGINT.
            // Darwin also reports terminal interrupts as user signals; the loop below
            // checks the shared foreground group to avoid delivering Ctrl-C twice.
            // The callback performs only a lock-free atomic store.
            handlers.push(unsafe {
                signal_hook_registry::register_sigaction(value, move |info| {
                    if info.si_code <= 0 && info.si_pid() > 0 {
                        signal.store(value as usize, Ordering::SeqCst);
                    }
                })
            }?);
        } else {
            handlers.push(signal_hook::flag::register_usize(
                value,
                signal.clone(),
                value as usize,
            )?);
        }
    }
    let mut command = Command::new(&args[0]);
    command.args(&args[1..]);
    let mut child = match engine.spawn_owned(&session.id, &mut command) {
        Ok(child) => child,
        Err(e) => {
            let _ = engine.end_session(&session.id, "launch_failed");
            for id in handlers {
                signal_hook::low_level::unregister(id);
            }
            return Err(e);
        }
    };
    eprintln!("Closingtime run: {}", session.id);
    let mut last_observation = Instant::now() - Duration::from_secs(1);
    let mut observation_failures = 0usize;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        let received = signal.swap(0, Ordering::SeqCst) as i32;
        if received != 0 && !(received == libc::SIGINT && shared_foreground_group(child.id())) {
            let current = engine.store.session(&session.id)?;
            if let Some(root) = current.root {
                let _ = engine.backend.signal(&root, received);
            }
        }
        if last_observation.elapsed() >= Duration::from_millis(250) {
            // A missed pass is a polling gap, not a false record: anything still running
            // is picked up by the next pass, and unrecorded processes are never stopped.
            if let Err(e) = engine.observe() {
                if observation_failures == 0 {
                    eprintln!(
                        "Ownership observation failed; retrying: {}",
                        safe(&e.to_string())
                    );
                }
                observation_failures += 1;
            }
            last_observation = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    for id in handlers {
        signal_hook::low_level::unregister(id);
    }
    // The final pass must succeed: a lost end leaves the run unconfirmed and report-only.
    let mut last = engine.observe();
    for _ in 0..3 {
        if last.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
        last = engine.observe();
    }
    use std::os::unix::process::ExitStatusExt;
    let code = status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1));
    let mut outcome = format!("exit:{code}");
    if observation_failures > 0 {
        outcome += &format!("; {observation_failures} observation passes failed");
    }
    if let Err(e) = last.and_then(|_| engine.end_session(&session.id, &outcome)) {
        return Err(format!(
            "run exited, but recording failed; cleanup stays blocked until `closingtime recover --session {}`: {e}",
            session.id
        )
        .into());
    }
    let scan = engine.scan(Some(&session.id))?;
    let count = scan
        .resources
        .iter()
        .filter(|r| r.status == "running" || r.status == "unrecorded")
        .count();
    eprintln!(
        "Run ended; {count} surviving processes. Preview: closingtime clean --session {}",
        session.id
    );
    Ok(code)
}
fn shared_foreground_group(child_pid: u32) -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::os::fd::AsRawFd;
        let group = libc::getpgrp();
        if libc::getpgid(child_pid as libc::pid_t) != group {
            return false;
        }
        // A foreground job can still receive Ctrl-C when stdin is a pipe/file.
        let foreground = [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO]
            .into_iter()
            .map(|fd| libc::tcgetpgrp(fd))
            .find(|&pid| pid > 0)
            .or_else(|| {
                std::fs::File::open("/dev/tty")
                    .ok()
                    .map(|tty| libc::tcgetpgrp(tty.as_raw_fd()))
            });
        foreground == Some(group)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = child_pid;
        false
    }
}
fn select(engine: &Engine<NativeBackend>, selector: &RunSelector) -> Result<Option<String>> {
    let sessions = engine.store.sessions()?;
    if selector.last {
        return match sessions.into_iter().max_by_key(|s| s.started_ms) {
            Some(s) => Ok(Some(s.id)),
            None => Err("no recorded runs yet".into()),
        };
    }
    let Some(query) = &selector.session else {
        return Ok(None);
    };
    let matches: Vec<_> = sessions
        .iter()
        .filter(|s| s.id.starts_with(query.as_str()))
        .collect();
    match matches.as_slice() {
        [one] => Ok(Some(one.id.clone())),
        [] => Err(format!("no recorded run matches {}", safe(query)).into()),
        _ => Err(format!(
            "{} runs match {}; use more characters",
            matches.len(),
            safe(query)
        )
        .into()),
    }
}
fn required(session: Option<String>) -> Result<String> {
    session.ok_or_else(|| "choose a run with --session <id> or --last".into())
}
fn json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn safe(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
fn warnings(items: &[String]) {
    for warning in items {
        eprintln!("Note: {}", safe(warning));
    }
}
fn show_scan(scan: &Scan, machine: bool) -> Result<()> {
    if machine {
        return json(scan);
    }
    println!("PID      RUN                               STATE          ACTION / PROCESS / PORTS");
    for r in &scan.resources {
        let ports = r
            .ports
            .iter()
            .map(|p| format!("{}:{}", p.address, p.port))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<8} {:<32}  {:<13} {} / {} / {}",
            r.identity.pid,
            r.session_id.as_deref().unwrap_or("unknown"),
            r.status,
            if r.cleanup_eligible {
                "eligible"
            } else {
                "keep/report"
            },
            safe(&r.name),
            safe(&ports)
        );
        println!("         Evidence: {:?}", r.evidence);
        if let Some(project) = &r.project {
            println!(
                "         Owner: {} / {} / {}",
                safe(r.command.as_deref().unwrap_or("registered harness")),
                safe(r.session_label.as_deref().unwrap_or("")),
                safe(project)
            );
        }
        if !r.reasons.is_empty() {
            println!("         {}", safe(&r.reasons.join("; ")));
        }
    }
    warnings(&scan.warnings);
    Ok(())
}
fn show_plan(plan: &CleanupPlan) {
    println!("Cleanup preview for run {}", safe(&plan.session_id));
    let scan = Scan {
        schema: plan.schema.clone(),
        resources: plan.resources.clone(),
        warnings: plan.warnings.clone(),
    };
    let _ = show_scan(&scan, false);
    println!(
        "{} eligible processes. Nothing has changed.",
        plan.resources.iter().filter(|r| r.cleanup_eligible).count()
    );
}
