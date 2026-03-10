# Issues — zero-from-scratch-redesign

## [2026-03-10] Starting fresh — no issues yet

## [2026-03-10] Task 20 verification blockers (pre-existing)

- `cargo test -- --test-threads=1` fails due to a pre-existing library test failure:
  - `infrastructure::processing::cpu::tests::process_frame_detects_solid_color_and_returns_center`
  - observed coverage is `0.003921569`, assertion expects `> 0.9`
- `cargo clippy -- -D warnings` fails due to pre-existing warnings/errors outside task scope:
  - dead code: `src/infrastructure/capture/dda.rs` field `output`
  - unnecessary cast: `src/application/recovery.rs`
  - `should_implement_trait`: `AppConfig::default()` in `src/domain/config.rs`
  - `useless_vec`: `valid_sources` in `src/domain/config.rs`
  - crate naming lint: `RoyaleWithCheese` non-snake-case
