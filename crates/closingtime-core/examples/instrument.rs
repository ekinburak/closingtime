//! Minimal embedding harness. This example records; it never applies cleanup.
use closingtime_core::*;
use std::{process::Command, time::Duration};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let program = args.first().ok_or("usage: instrument COMMAND [ARG ...]")?;
    let engine = Engine::new(
        Store::open(&default_state_dir()?, true)?,
        NativeBackend::new()?,
    );
    let project = std::env::current_dir()?.to_string_lossy().into_owned();
    let session = engine.begin_session("example harness", &project, None)?;
    let mut command = Command::new(program);
    command.args(&args[1..]);
    let mut child = engine.spawn_owned(&session.id, &mut command)?;
    while child.try_wait()?.is_none() {
        engine.observe()?;
        std::thread::sleep(Duration::from_millis(250));
    }
    engine.observe()?;
    engine.end_session(&session.id, "example root exited")?;
    println!(
        "{}",
        serde_json::to_string_pretty(&engine.plan_cleanup(&session.id)?)?
    );
    Ok(())
}
