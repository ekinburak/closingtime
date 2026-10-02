# Closingtime open-source core requirements

Status: local prototype implementation. Owner: Ekin Burak. Date: September 30, 2026, Europe/Athens. Working name: Closingtime; name availability is not checked.

## Decision

Build a small offline CLI and ownership library for Linux and macOS. Cover processes and their TCP listening ports. Keep session ownership, local history, preservation controls, and cleanup safety in the Apache-2.0 core. Team services, pricing, and buyer validation are deferred to TODO.md.

Success means a developer or harness can distinguish concurrent runs, explain recorded survivors, preserve intentional background work, and review cleanup that refuses uncertain targets.

## Research corrections

The original `agent-leftovers-prd.docx` is a discovery brief, not the implementation scope. Its proposed scanner, recovery engine, hooks, MCP, six resource classes, and fleet roadmap exceed the recommended small library.

Portlist already persists ownership observations and provides reviewed cleanup. Corral already provides spawn-time containment on Linux. Closingtime's proposed contribution is a reusable session identity and explicit registration interface that other tools can consume; the ownership ledger itself is not novel. Upstream adoption is not established.

GitHub issue estimates in the source document come from classification and small hand-checked samples. Its referenced raw datasets are absent from this folder, so their totals are not independently reproducible here. Low stars or HN scores do not prove low installation or retention. The defensible team-market statement is that the research did not establish a buyer or a validated offering, not that no competitor exists.

Current official Claude Code documentation gives SessionEnd a 1.5-second default budget with configured extensions up to 60 seconds. Codex documents a one-second default and three-second maximum. These are version-sensitive facts, not universal hook guarantees. Hooks can provide lifecycle metadata later; cleanup does not run in hooks.

Primary references: [Portlist](https://github.com/Mr-hunt-007/portlist), [Corral](https://github.com/Cardinal44/corral), [Claude hooks](https://code.claude.com/docs/en/hooks), [Codex hooks](https://learn.chatgpt.com/docs/hooks). The original document retains the broader evidence and caveats.

## Required behavior

- Every command launch has a fresh run identity; native agent IDs are separate optional metadata.
- Process identity includes host, boot, effective UID, PID, and kernel start time. Linux also distinguishes PID namespaces. Ports belong to the current process identity, not a permanent port number.
- SQLite persists sessions, observations, keep decisions, and action results. Ownership survives reparenting. Conflicts remain conflicts.
- Read operations show current OS facts without changing ownership or decisions. Missing metadata is distinguishable from an absent resource.
- The wrapper registers its root around launch and observes descendants while supervising it. Launch registration, inherited tags, and ancestry are distinct evidence. It does not claim complete spawn interception.
- Library users can begin/end a run, spawn/register an owned process, observe resources, preview cleanup, and apply a reviewed plan. JSON exports use `closingtime.session.v1`.
- Cleanup eligibility requires confirmed lifecycle end, recorded ownership, current identity/executable agreement, readable metadata, known unmanaged status, and no protection or keep.
- Cleanup preview is the default. Apply requires terminal review, fresh checks, and durable logging. There is no noninteractive apply or automatic cleanup.
- Keep covers the known descendant graph; uncertain lineage is report-only when a keep could apply.
- Manager-owned processes, zombies, other users' processes, system/IDE/terminal main processes, Closingtime, and invoking ancestors are excluded.
- Apply sends individual SIGTERM, waits three seconds, and rechecks before individual SIGKILL. Linux requires pidfds. macOS retains a documented check/signal race.
- Interrupted or failed observations never manufacture an ended session. A missing end event blocks cleanup until a harness explicitly verifies and records completion.

## Acceptance and release gate

The suite must cover same-project concurrent runs, tagged double-forks, manual/unknown resources, active or unconfirmed runs, kept descendants, PID and boot changes, permission failures, zombies, manager ownership, stale reviewed plans, failed ledger writes, and concurrent writers. Real platform fixtures must confirm TCP association, exit forwarding, read-only behavior, and escalation on an owned process ignoring SIGTERM.

Maintain at least 200 labelled decisions with zero incorrectly eligible targets; publish coverage separately. Target list/lookup under one second with 200 recorded identities. Model timing and native collector timing are separate measurements.

October 1–14 is the prototype window, not a public-launch promise. A public alpha additionally requires native checks on both systems and dogfooding interactive Claude Code/Codex sessions. Building binaries locally does not constitute publishing or live-agent validation.
