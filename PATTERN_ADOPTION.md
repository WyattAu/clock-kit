# Pattern adoption: clock-kit

Relationship of this crate to the WyattAu pattern canon
(`pattern-standards/references/catalog.md`). Validated by
`pattern-standards/scripts/adoption-check.py`.

| Pattern | Status | Evidence |
|---|---|---|
| P8 | adopted | src/timestamp.rs:74 (Timestamp carries wall and mono with independent step rules), src/mock.rs:198 (step_wall moves wall without touching mono), src/timestamp.rs:95 (sole cross-derivation, labelled lossy) |
| P20 | adopted | src/clock.rs:24 (single Clock trait serves production and tests), src/mock.rs:3 (MockClock never consults the OS; replay is reproducible), src/mock.rs:222 (MockClock implements that trait) |
| P64 | adopted | tests/latency_ring.rs:62 (proptest suite), tests/monotonic.rs:14 (mono never regresses under all sources) |
| T36 | partial | src/monotonic.rs:135 (loom model check of the mono floor), src/monotonic.rs:194 (regression clamp tests; scoped to the floor module, the rest of the crate is not simulation-tested) |
