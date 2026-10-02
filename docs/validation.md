# Prototype validation — September 30, 2026

These are local results for the source in this folder. No hosted CI run, public release, external integration, or real coding-agent workload is claimed.

## Environments and results

| Check | Native macOS 27.0.1, Apple Silicon | Linux ARM64, `rust:1.98-bookworm` in Docker |
|---|---|---|
| Rust | 1.98.1 | 1.98.1 |
| Formatting | Passed | Passed |
| Clippy, all targets, warnings denied | Passed | Passed |
| Core tests | 17 passed | 17 passed |
| Native test entries | 6 passed, including the worker entrypoint | 6 passed, including the worker entrypoint |
| Ctrl-C with terminal stdin | Passed, exactly one child interrupt | Passed, exactly one child interrupt |
| Ctrl-C with redirected stdin | Passed, exactly one child interrupt | Passed, exactly one child interrupt |
| Interactive cleanup | Reviewed owned fixture stopped; result recorded | Reviewed owned fixture stopped; result recorded |
| List with 200 live recorded processes | 180 ms | 123 ms |
| PID lookup with those 200 live processes | 167 ms | 122 ms |

Timing is one native collector + ledger measurement per operation in the final suite run, using bounded `/bin/sleep` fixtures. It meets the one-second target on these environments. It does not establish a sustained-load benchmark, a percentile, performance on every machine, or macOS 14 compatibility. The CI matrix targets macOS 14 and Ubuntu 22.04 but has not run in a hosted repository.

The macOS release CLI was built locally at `target/release/closingtime`. Linux compilation and native execution were verified inside the disposable container. Cargo dependencies were resolved from the locked local cache; the Linux Rust checkers were installed into the container. The CLI itself makes no network calls.

## Labelled eligibility corpus

`labelled_safety_corpus_240_cases` constructs 20 identity/session variants in each category below. These are synthetic fixtures with explicit expected labels, not independently collected production incidents or 240 different failure mechanisms.

| Category | Cases | Expected eligible |
|---|---:|---:|
| Recorded, matching, ended, unkept process | 20 | 20 |
| Kept process | 20 | 0 |
| PID start-time reuse | 20 | 0 |
| Previous boot | 20 | 0 |
| Other user's process | 20 | 0 |
| Unreadable ownership metadata | 20 | 0 |
| Zombie | 20 | 0 |
| Manager-owned service | 20 | 0 |
| Unknown manager status | 20 | 0 |
| Conflicting current tag | 20 | 0 |
| Recorded sticky ownership conflict | 20 | 0 |
| Changed executable | 20 | 0 |

Result on each platform: **20 expected eligible, 220 expected blocked, zero incorrectly eligible and zero incorrectly blocked labelled records**. Forty additional current identities discovered by the reuse/boot variants were reported as unrecorded and ineligible. This result is not a universal cleanup safety guarantee.

Separate core checks cover two concurrent runs in one project, tagged double-forks, keep inheritance after reparenting, uncertain keep lineage, manual/unknown processes, active runs, missing end events, unreadable or reappearing roots, protected invoking ancestry, stale plans, unavailable pidfds, failed action writes, sticky manager ownership, stale concurrent observations, and eight writers recording 80 runs.

Native fixtures exercise actual detached TCP listeners, ownership lookup, unchanged preview records, verified cleanup, a process ignoring SIGTERM that needs SIGKILL, child exit-status forwarding, refusal of piped apply, and read commands with no existing ledger. Fixture workers expire after 30 seconds, and tests signal only identities they launched.

## Agent executable checks and remaining gates

Wrapping `claude --version` (Claude Code 2.1.98) and `codex --version` (Codex CLI 0.159.2) succeeded on macOS. Their ledger contained two distinct ended runs and no cleanup actions. This confirms executable launch and recording; it does not validate interactive agent work, long-running servers, hooks, or agent-specific descendants.

Before public alpha, complete the real agent dogfooding, hosted platform matrix, broader independent safety review and release decisions in [TODO.md](../TODO.md). macOS's identity-check/signal race remains a documented limitation; the Linux path requires pidfds.

## Local command installation and fresh recheck

After separating the private website, the unchanged Rust source was checked again on this Mac. Formatting, Clippy with warnings denied, all 17 core tests, all six native test entries and the terminal smoke checks passed. These are fresh macOS results; the Linux results above are from the earlier container run.

The release CLI was installed locally at `~/.cargo/bin/closingtime`; `closingtime --version` reports `0.1.0`. The command is available on the existing shell PATH. The build used the previously populated dependency cache and the locked dependency versions, offline.

The installed executable passed `doctor` with process identity, environment metadata and TCP inventory available. It also passed terminal Ctrl-C checks, including redirected stdin, and interactive reviewed cleanup with recorded outcomes. A separate bounded listener check exercised TCP ownership lookup, JSON keep/unkeep decisions and active-run exclusion through the installed command. Verification used isolated temporary ledgers and test-owned processes.

No source defect was found during this recheck. This local installation is not a published package, hosted CI result or completion of the real agent workload release gates.

## Binary packaging check — October 1, 2026

The native packaging script built `closingtime 0.1.0` for
`aarch64-apple-darwin` with locked dependencies. The resulting local archive's
SHA-256 checksum, extracted executable permissions and version were verified.
The extracted binary passed `doctor`, both terminal Ctrl-C cases and the
interactive cleanup fixture with an isolated ledger and test-owned processes.

This verifies the archive on the current Mac, not older macOS compatibility,
Linux archives, signing/notarization or public distribution. The manual
packaging workflow has not run in hosted CI. Public releases and Homebrew remain
pending; see [distribution preparation](distribution.md).

## Initial source publication checks — October 2, 2026

Before the initial source commit, the existing verification commands passed on
macOS 27.0.1 with Rust 1.98.1: formatting, Clippy across all targets with warnings
denied, all 17 core tests, all six native test entries, and the three terminal
smoke checks. The terminal checks covered Ctrl-C with normal and redirected
stdin, plus interactive cleanup of a bounded test-owned fixture.

The new README artwork and navigation passed local browser checks in light and
dark themes, at desktop and mobile widths, and with reduced motion enabled.
Images and local links resolved without page overflow. These browser previews
are local evidence, not a capture of GitHub's rendered page.

Source publication does not complete real agent workload testing or binary
release gates. Hosted results are tracked separately in
[GitHub Actions](https://github.com/ekinburak/closingtime/actions/workflows/ci.yml).
