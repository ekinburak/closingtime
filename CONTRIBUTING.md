# Contributing to Closingtime

Closingtime is an early offline CLI and Rust library. Contributions should help explain recorded process ownership, preserve intentional background work, or make reviewed cleanup more reliable.

For a bug or proposal, [open an issue](https://github.com/ekinburak/closingtime/issues). Describe the concrete behavior and, when possible, include a minimal reproduction. For a larger feature, discuss the scope before investing in an implementation.

## Build and verify

Use Rust 1.85 or newer and a native C toolchain. Python 3 is needed for the terminal smoke checks.

```sh
cargo build --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 tests/terminal_smoke.py --binary target/debug/closingtime
```

The smoke checks launch short-lived test processes. Read [the validation record](docs/validation.md) for the distinction between synthetic decisions, native fixtures and real agent workloads.

## Keep the ownership and review boundary

- Leave missing, conflicting or unreadable evidence report-only.
- Keep attribution separate from permission to stop a process.
- Preserve keeps, active runs and protected processes.
- Recheck process identity and eligibility before signaling.
- Include a relevant reproduction or fixture when changing safety behavior.
- Keep a pull request focused on its concrete problem.

[PRD.md](PRD.md) defines the current processes-and-TCP-ports scope. [TODO.md](TODO.md) records deferred work. Hooks, automatic cleanup, additional resource classes and team services are not existing capabilities.

## Docs and artwork

Check relative links and command examples when editing documentation. Label illustrative visuals as examples. Do not turn local test results into claims of hosted CI or production readiness.

The README logo uses the existing website identity. Its light/dark wordmarks and illustrative animation are generated from [the asset source](scripts/readme-assets.py); see [asset notes](docs/assets/README.md) for fonts, licenses and regeneration.

The project uses [Apache-2.0](LICENSE). The bundled asset fonts retain their separate OFL licenses.
