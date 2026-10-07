# Closingtime deferred work

The active scope is the offline processes-and-ports CLI and library in PRD.md. Items below are not promises of existing capabilities.

## Before a public OSS alpha

- [ ] Dogfood real interactive Claude Code and Codex runs on Linux and macOS, including concurrent projects, crashes, retained servers, and MCP children. Version commands alone do not validate an agent workload.
- [ ] Run the CI matrix in a hosted repository; local checks are not a CI result.
- [ ] Repeat native list/lookup latency checks on representative developer machines and measure sustained observation overhead. The current suite includes 200 live recorded processes; model timing remains separate.
- [ ] Expand independently labelled safety scenarios and review the unsafe platform boundary.
- [ ] Decide whether macOS's residual PID reuse race is acceptable for alpha cleanup or warrants report-only distribution.
- [x] Recover runs with missing end events only after verifying the supervisor and root exited (`closingtime recover`, `recover_session`); no unverified 'mark ended' shortcut.
- [ ] Check project/crate name availability, repository location, release packaging and support policy.
- [ ] Add signed/checksummed release binaries and a Homebrew tap only after release approval.
  - [x] Prepare a native archive/checksum builder and a manual CI artifact workflow. Public downloads, hosted execution, signing and Homebrew remain pending.

## OSS integrations and stronger instrumentation

- [ ] Discuss the export contract with Portlist, Herdr, Leftovers, cc-reaper, and Corral maintainers. No outreach has been sent.
- [ ] Submit an integration only after a maintainer agrees on scope; no adoption is assumed.
- [ ] Add supplementary Claude/Codex hooks with version-specific compatibility tests and atomic configuration backups.
- [ ] Investigate reliable per-descendant launch registration, wrapper-independent crash observation, and lower-overhead observation.
- [ ] Add manager-aware stops, explicit Docker labels, and container ownership.
- [ ] Design Linux cgroup/subreaper containment with keep semantics before implementing group cleanup. Do not promise cgroup.kill on Linux 5.4.
- [ ] Investigate macOS Endpoint Security only if its entitlement and deployment requirements are justified.
- [ ] Add MCP read/plan tools and a harness-owned approval contract before apply.

## Additional resource classes

- [ ] Worktrees: preserve dirty/untracked/ignored content; model in-use state, commit reachability and squash merges; design and test recovery before removal.
- [ ] Temporary files and directories: establish explicit ownership, quarantine identity, TTL and recovery before deletion.
- [ ] Scheduled jobs: explicit registration and manager-aware lifecycle handling.
- [ ] Windows: Job Objects, identity collectors and native safety fixtures.
- [ ] Cloud sandbox inventory: read vendors' full lifecycle documentation, validate API support and distinguish sandbox stop from external-resource cleanup.
- [ ] Semantic residue: provenance export only until an agent harness owns reauthorization; do not claim credential revocation or containment.

## Future team and commercial hypotheses

- [ ] Validate shared agent hosts first; self-hosted CI is a secondary hypothesis.
- [ ] Interview eight infrastructure leads about concrete recent incidents, recurrence, workaround, cost and installation constraints.
- [ ] Revisit the original two-week commercial validation schedule rather than assuming it is underway.
- [ ] Stop the paid track if fewer than three of eight report recurring pain; require actual trial commitments before building a control plane.
- [ ] Test multi-host inventory, centralized policies, access controls and durable team audit history with design partners.
- [ ] Separate the complete free safety/ownership core from any later proprietary team service.
- [ ] Evaluate hosting, support, pricing, billing, security obligations and willingness to pay later. No price is chosen now.
- [ ] Do not treat downloads, stars, funding or launch attention as paid-demand validation.
