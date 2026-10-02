# Closingtime usage and safety guide

Record what a coding-agent run starts, explain what survives, and review cleanup before stopping anything.

Closingtime is an offline Rust CLI and library for Linux and macOS. It records session ownership for processes and derives their TCP listening ports from current OS observations. The Apache-2.0 core needs no account, subscription, or telemetry.

This is an early local prototype. It does not intercept every spawn, enforce a sandbox, or guarantee that all descendants can be found. Unknown ownership stays unknown.

## Build and use

```sh
cargo build --release --locked
# Or install the CLI locally from this checkout:
cargo install --path crates/closingtime-cli --locked

closingtime doctor
closingtime run --label checkout -- claude
closingtime run -- codex
closingtime sessions
closingtime scan --json
closingtime who --port 3000
closingtime who --pid 12345
closingtime keep --pid 12345
closingtime clean --session RUN_ID
closingtime clean --session RUN_ID --apply
closingtime unkeep --pid 12345
closingtime export --json
```

`run` inherits the terminal and returns the command's exit status. It records a separate run ID for every invocation, sets `CLOSINGTIME_SESSION` and `CLOSINGTIME_PROJECT`, and observes descendants every 250 ms while the root runs. Observation writes are batched. It performs a final observation after the root exits. Nothing is automatically reaped.

Terminal Ctrl-C reaches the child through its foreground process group, without a duplicate forwarded interrupt. On macOS, an explicitly sent SIGINT to the wrapper while it shares that foreground group cannot be distinguished from terminal Ctrl-C and is not forwarded. Use SIGTERM to interrupt the wrapper externally; SIGINT is forwarded when there is no shared foreground terminal group.

Processes registered by the library, observed with inherited tags, and observed through ancestry have separate evidence labels. A process discovered after the supervisor stopped can appear as **unrecorded**, with cleanup disabled. Closingtime never assigns ownership from the project directory alone.

Read commands and cleanup previews do not update ownership or decisions. They also work with an empty ledger without creating a state directory. `export` always prints JSON. Other read commands accept `--json`.

## Review and safety

`clean` is a preview. `clean --apply` refreshes it and asks you to type `yes` in a terminal. Pipes and JSON mode cannot apply cleanup. The CLI has no `--yes`, automatic cleanup, process-name kills, process-group kills, or whole-cgroup kills.

Each target must have a recorded owner, a confirmed ended run, a matching current kernel identity and executable, readable ownership metadata, known unmanaged status, and no keep or protection rule. The check runs again before **each signal**. An application cannot clean an active run merely because it knows its owner.

Keep decisions cover known descendants even after reparenting. If a detached process has incomplete ancestry and its run contains a keep, it stays report-only. Removing a child's explicit keep does not override a kept ancestor.

Closingtime signals one verified process at a time: SIGTERM, a three-second grace period, then SIGKILL if it remains eligible. Linux uses pidfds and refuses to fall back to an ordinary PID kill. macOS rechecks kernel start time immediately before signaling, but Darwin has no pidfd equivalent: a remaining PID reuse race is documented, not claimed away. Zombies are reported as awaiting parent reaping.

Other users' processes, Closingtime, its invoking ancestors, system processes, known IDE/terminal main processes, and manager-owned services are excluded. A recorded positive manager observation is preserved for that identity and its observed descendants. Unavailable launchd or service metadata blocks eligibility. Manager-aware stops are future work.

Missing end events, failed observations, and forced supervisor crashes leave the run unconfirmed and report-only. An embedding harness can call `end_session` after verifying its root exited. The CLI does not guess that an abandoned run is safe to reap.

Cleanup writes durable action intent before sending a signal. If recording fails, it stops without sending further signals. Action results and evidence are available in `export`. The local SQLite history is **not** an immutable compliance log.

## State and privacy

Default state directory:

- Linux: `$XDG_STATE_HOME/closingtime`, or `~/.local/state/closingtime`.
- macOS: `~/Library/Application Support/closingtime`.

Use `--state-dir PATH` or `CLOSINGTIME_STATE_DIR` for a dedicated private directory. Directories must be owned by the current user with mode 0700; ledger files must be private regular files. SQLite uses WAL, a busy timeout, and full synchronous writes. Existing arbitrary directories are not silently made private.

Stored metadata includes project paths, executable paths, process identities, labels, run outcomes, and cleanup evidence. Full arguments, prompts, transcripts, arbitrary environment variables, credentials, and network payloads are not stored. The OS collector reads process environment metadata to extract the session tag. Data stays local; the tool makes no network calls.

Tags and the library registration API provide provenance in a cooperative developer workflow. They are not authentication or an adversarial security boundary; a program running as the same user can alter local state or copy tags.

Linux identity includes the PID namespace because containers can share a kernel boot ID. Platforms are Linux with `/proc` and pidfd support for cleanup, and macOS 14+. Permission restrictions can reduce collection or make cleanup unavailable; `doctor` reports those limitations.

## Integrate

The `closingtime-core` crate exposes `begin_session`, `spawn_owned`, `register_owned`, `observe`, `end_session`, `plan_cleanup`, `apply_plan`, and keep controls. The CLI uses the same library.

- [Rust harness example](../crates/closingtime-core/examples/instrument.rs)
- [Python export reader](../examples/python_reader.py)
- [Versioned export contract](session-v1.md)

An embedding harness must provide its own review and approval before constructing `ReviewedApproval`. That type acknowledges a caller decision; it is not an authorization token. Importing an export to authorize cleanup is not supported.

## Verify

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tests/terminal_smoke.py --binary target/debug/closingtime
```

Tests include a labelled 240-case safety corpus and real OS fixtures for detached processes, TCP port ownership, SIGTERM escalation, exit-status forwarding, terminal Ctrl-C, interactive approval, read-only behavior, refusal of piped cleanup, and list/lookup latency with 200 live recorded processes. Test workers expire after 30 seconds and act only on fixture identities. Model tests separately cover boot/PID changes, unreadable metadata, manager uncertainty and recorded management inheritance, keep changes after review, protected ancestors, missing ends, concurrent writers, and failed audit writes.

Zero incorrect eligible targets in a corpus is evidence for those scenarios, not a universal safety guarantee. Native collectors and live agent workflows need ongoing dogfooding before a public alpha.

[Local validation results and coverage](validation.md) distinguish the synthetic corpus, native fixtures, performance measurements, executable smoke checks, and remaining release gates.

Private discovery research is outside the source distribution. [PRD.md](../PRD.md) is the implementation scope; [TODO.md](../TODO.md) contains deferred product and commercial work. No integrations, outreach, pricing, or paid service are implied to exist.
