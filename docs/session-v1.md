# closingtime.session.v1 export contract

`closingtime export --json` emits one UTF-8 JSON object. There is no import or remote API in v0.1. Consumers should reject an unsupported `schema` and tolerate additional fields within version 1; semantic breaking changes require a new version.

Top-level fields are `schema`, `sessions`, `processes`, `kept`, and `actions`. The Rust types in `closingtime_core::model` define the exact serialized fields.

| Record | Meaning |
|---|---|
| Session | `id`, label, program name (`command`, never arguments), project, optional native ID, host/boot/UID, supervisor/root identities, active/ended state, timestamps and outcome. A run ID identifies an invocation, not an entire resumed agent conversation. |
| Process identity | `host`, `boot`, `uid`, `pid`, `start`. Treat all five as one key. Linux start is kernel ticks; macOS start is seconds:microseconds. Never compare start units across systems. |
| Ownership record | Identity, owning run ID, optional parent identity key, executable/name, evidence, observation timestamps, sticky `conflict` and `manager_owned` flags. The executable name is metadata, not a cleanup selector. Concurrent observations merge safety flags and known parent lineage rather than overwrite them. |
| Keep | A set of serialized identity keys. A key is JSON serialization of the identity object; consumers should parse it rather than split it on punctuation. Ancestor keep propagation is evaluated against recorded parent relationships. |
| Action | Target identity, run, actor UID, timestamps, evidence, requested signals and current result. `pending` means intent was recorded but final outcome is unknown. It does not prove the signal was sent. |

Evidence values: `launch_registration`, `explicit_registration`, `inherited_tag`, `observed_ancestry`. These are observed sources, not numerical safety scores. Tags can be copied by same-user programs and are not authorization credentials.

Read command envelopes and cleanup plans also carry `schema`. Resource views contain current ports, status, keep state, `cleanup_eligible` and refusal reasons. A listening port maps to its enclosing process identity only at observation time. An export contains historical ownership, not a current port inventory; use `scan --json` for current ports.

`cleanup_eligible` in a captured scan or plan is advisory. A harness must review the plan, then use the library apply operation, which rechecks eligibility and identity. Reading exports or constructing an approval acknowledgment does not bypass those checks.

The source of integration data is the supported CLI/library, not SQLite table names. Project/executable paths are local metadata; integrations must obtain consent before transmitting them. Closingtime itself transmits nothing.
