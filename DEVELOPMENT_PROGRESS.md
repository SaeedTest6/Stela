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

Muse audit (2026-11-23 evening, continuing from `6ac8b29`): no Rust toolchain
is installed on the Muse VM at all, and the local clone must not be built or
run there (execution-risk check on the cloned content). Muse therefore audited
by reading code only and did not run any test locally. Validation of any future
change must come from GitHub CI. Note: `.github/workflows/ci.yml` triggers on
push to `main`, pull requests and `workflow_dispatch` only — a push to this
development branch alone runs no CI. To get CI results for a change on this
branch, open a PR against `main` (do not merge it) or dispatch the workflow.

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

Muse audit, 2026-11-23 evening (read-only, no defect found):

- `stela-compiler/src/aarch64/flags.rs` + `src/x86_64/flags.rs`
  (NZCV condition evaluation, FlagM/FlagM2 CFINV/RMIF/SETF8/16, AXFLAG/XAFLAG,
  MRS/MSR NZCV): `condition_holds` matches the architectural truth table for
  all 16 conditions x 16 NZCV nibbles (test
  `every_condition_matches_the_architectural_truth_table`); the AXFLAG/XAFLAG
  conversion tables are checked against all flag inputs
  (`external_table_matches_all_architectural_flag_inputs`); MRS/MSR NZCV
  complete inline in both SYSTEM_MODE and ORDINARY profiles
  (`nzcv_reads_and_writes_complete_inline_in_every_profile`). No defect found.
- `stela-compiler` integer division (`aarch64/integer_math.rs` decode +
  `x86_64/integer_math/division_remainder_tests.rs`): SDIV/UDIV decode is
  exact (reserved neighbours never decode as another divide); the
  divide+MSUB grouping refuses aliased inputs and unmatched rows; the
  state-comparison tests cover divisor 0 (quotient 0, remainder = dividend),
  INT_MIN / -1 and 32/64-bit signed/unsigned forms, including independent
  entry at the MSUB row and pinned/system-mode recovery placement. The x86
  IDIV trap cases are therefore guarded by construction and by test.
  No defect found.
- `stela-contracts/src/bus.rs` + `bus/container.rs`, unchecked adds
  (follow-up to the note above, proof): `route()` computes
  `region.device_offset + (address - region.start)` and `shared_view()`
  computes `region.device_offset + (cursor - region.start)` without
  `checked_add`. Both are provably non-overflowing for every state reachable
  through the public API: `regions` is documented and maintained sorted by
  `start` and non-overlapping (`install_many` sorts; `map` inserts at the
  `partition_point`); `install`/`map` both reject with `MapError::Overflow`
  unless `device_offset + len` fits, where `len = region.end - region.start`;
  in `route()` the address lies inside one region, so the delta is `< len`;
  in `shared_view()` the cursor only ever equals a region's start (later
  regions) or lies inside the first overlapping region with
  `cursor - region.start < len` (earlier regions cannot overlap it). The
  sums are therefore `<= device_offset + len`. No change made; a
  `checked_add` here would only defend against direct in-crate mutation of
  `regions` (as existing tests do), which is not a reachable production
  state. Revisit if `regions` ever stops being sorted/non-overlapping.
- `stela-engine/src/blocks.rs` `BlockRuntime` (cache invalidation):
  the dispatch path is generation-checked — a loaded block only runs when
  `block.generation == current.generation` ("refusing stale block execution"),
  and same-base `register_code` advances the generation and drops the loaded
  blocks for that base. The production machine path (`machine/engine/`) has
  its own invalidation: `code_cache.rs::invalidate_code_page`, called from
  `device_access.rs` on writes and `native_failure.rs` on faults, plus
  `boundary.rs::invalidate_translations` / `code_invalidated` (IC IVAU) and
  the parallel mailbox invalidation in `parallel.rs`. No defect found there
  in the parts read.
  FINDING (latent, BlockRuntime only): `BlockRuntime::source_overlaps` —
  documented as the guard "a permission service must not bypass
  immutable-source visibility or invalidation by presenting registered code
  as an ordinary data mapping" — has **zero callers anywhere in the repo**
  (verified by grep over all `.rs` and `.md` files), and `unregister_code`
  is called only from tests. `BlockRuntime` itself is used only by
  `stela-dev`, examples and tests, not by `stela-runtime`'s machine path.
  Consequence: a `BlockRuntime` consumer that lets the guest write to a
  registered code region through a data mapping will silently keep executing
  the old native block — the snapshot never changes and the generation
  never advances, so the dispatch check cannot see the write. Severity:
  medium for `BlockRuntime` as a component, low for the product (the real
  boot path does not use it and has its own invalidation). Fix: either wire
  `source_overlaps` into the checked-memory permission path of
  `BlockRuntime` consumers so a data write overlapping registered code is
  refused or invalidates, or document it as dev-only and delete it; proving
  test: register code, write through a data mapping overlapping it, then
  dispatch — expect refusal/invalidation and the new bytes executed.
- `stela-soc/src/aic.rs` (header/documentation, ~120 lines read): AICv2/v3
  layout, event-register acknowledge-and-mask semantics, die/IRQ vector
  numbering and the AICv3 relocated arrays are documented against Linux
  `irq-apple-aic.c`, m1n1 `aic.c` and 26A434 kernelcache drivers, with
  inferred (undocumented) behaviour explicitly labelled as inferred and
  cross-die arbitration stated as a deterministic permitted ordering, not a
  documented contract. No defect found in the part read; the register
  implementation below the header was not audited.

Not audited (do not read this as "correct"):

- Everything else in stela-compiler: flags/condition mapping in x86_64
  codegen, division, SIMD/FP, atomics, AMX/SME/SVE. (Flags decode, NZCV
  codegen and division are now audited — see above. Still open: SIMD/FP
  codegen, atomics, AMX/SME/SVE, and the lazy-flags / resident-loop paths.)
- stela-engine: code cache invalidation, fault recovery, register sync,
  parallel execution, the `unsafe` blocks. Only `execution/checked_memory.rs`
  was skimmed; it is a thin runner and the access checks live in generated
  code and `machine/`. Exit validation (`blocks/validate.rs`) is now
  audited — see above. Cache invalidation is now partly audited — see the
  `BlockRuntime` finding above (production `machine/engine` invalidation
  paths read, `source_overlaps` dead-code finding recorded). Still open:
  fault recovery, register sync, parallel execution, `unsafe` blocks.
- stela-soc: AIC, PMGR, ANS, DART and all other device models beyond
  `regfile`/`amcc`. (AIC header documentation now read — see above. Still
  open: AIC register implementation, PMGR, ANS, DART.)

Three parallel read-only audit agents were started for the three crates and
all three were cut off by an API rate limit before returning any finding, so
no agent findings exist and none are recorded here.

## Open items

- Obtain the baseline for stela-compiler and stela-engine (long builds/tests).
- Environment: a 1.99.1 toolchain is needed to reproduce CI results exactly.
- Targeted audits listed under "Not audited", each with a failing test before
  any fix.
- Coordination: Muse and Claude have both worked this branch on 2026-11-23.
  Before pushing anything, confirm who is active; HEAD was `6ac8b29` when
  Muse's audit started and was unchanged when re-checked the same evening.
  A push to this branch runs no CI by itself (see Environment note).

## Decisions

- No architecture change was made or proposed.
- Existing tests were not modified or removed.
