# STELA development progress

Branch: `development/compiler-engine-soc` (never `main`, never the private `Stela2`).
Scope: `stela-compiler`, `stela-engine`, `stela-soc`.
Rules: correctness first; one commit per behavioral change; every change has a
test; no mock presented as hardware evidence; unknown behavior refuses.

Feature status vocabulary: Implemented / Partially implemented / Simulated /
Unsupported / Refused safely.

## Environment note (read first)

`rust-toolchain.toml` pins 1.99.1 and the workspace sets `rust-version = "1.99"`.
The audit sandbox has only Rust 1.97.0 and cannot download 1.99.1 (the
egress proxy blocks static.rust-lang.org). Local runs therefore used

    RUSTUP_TOOLCHAIN=stable cargo test --locked --ignore-rust-version -p <crate>

`--ignore-rust-version` is a local workaround only; nothing in the repository
was changed for it. Results below are 1.97 results, not 1.99.1 results.
`cargo clippy` and `cargo fmt --check` for the whole workspace were not run;
`cargo fmt -p stela-soc -- --check` was clean after the change below.

## Baseline (before this session's code change)

| Crate | Result |
| --- | --- |
| stela-contracts | 65 passed, 0 failed |
| stela-soc-data | 184 passed, 0 failed, 23 ignored |
| stela-soc | 1618 passed, 0 failed, 79 ignored |
| stela-compiler | not completed in this session (see Open items) |
| stela-engine | not run in this session (see Open items) |

## Change log

1. `3c9087b`, `52652dd` (pre-existing, kept): `regfile::role` refuses an offset
   covered by two spans instead of taking the first. Note: `3c9087b` is the
   behavior change; `52652dd` only makes its test order-independent.
2. `c52041f` (pre-existing, kept): workspace manifest loads under edition 2024.
3. `7470d27`: `regfile::first_overlap` (const) reports the first pair of span
   descriptors sharing an offset; `amcc::tests::shipped_span_tables_never_overlap`
   asserts it for `T8112_PLANE` and `T8112_SHARED`. Both tables are disjoint, so
   the ambiguity refusal removes no register from the model today. stela-soc:
   1620 passed, 0 failed (was 1618).

## Audit coverage so far

Read in full or in the relevant part, no defect found:

- `stela-soc/src/regfile.rs` and its only users, the two span tables in
  `amcc.rs` (the only module that uses `regfile::Span`/`role`). Table entries
  checked by hand for overlap; now checked by test.
- `stela-contracts/src/bus.rs` (map/install/route) and `bus/container.rs`: all
  range arithmetic uses `checked_add`; overlap is rejected at map time.
  One unchecked add in `shared_view`
  (`region.device_offset + (cursor - region.start)`) is bounded by the
  `checked_add` done at map time; not changed.
- `stela-compiler/src/aarch64.rs` `Decoder::lift` for the base integer classes
  (move wide, logical immediate and `logical_mask` against Arm's
  DecodeBitMasks, add/sub immediate/shifted/extended/carry, scalar load/store
  size/opc reservations, CBZ/TBZ, B): reserved encodings (MOVZ/MOVN hw>=2 with
  sf=0, ROR in add/sub, 32-bit shift amount >= 32, N=1 for W32 logical
  immediates, unprivileged forms, PRFM) are refused. No defect found.

Not audited (do not read this as "correct"):

- Everything else in stela-compiler: flags/condition mapping in x86_64
  codegen, division, SIMD/FP, atomics, AMX/SME/SVE.
- stela-engine: code cache invalidation, fault recovery, register sync,
  parallel execution, the `unsafe` blocks. Only `execution/checked_memory.rs`
  was skimmed; it is a thin runner and the access checks live in generated
  code and `machine/`.
- stela-soc: AIC, PMGR, ANS, DART and all other device models beyond
  `regfile`/`amcc`.

Three parallel read-only audit agents were started for the three crates and
all three were cut off by an API rate limit before returning any finding, so
no agent findings exist and none are recorded here.

## Open items

- Obtain the baseline for stela-compiler and stela-engine (long builds/tests).
- Environment: a 1.99.1 toolchain is needed to reproduce CI results exactly.
- Targeted audits listed under "Not audited", each with a failing test before
  any fix.

## Decisions

- No architecture change was made or proposed.
- Existing tests were not modified or removed.
