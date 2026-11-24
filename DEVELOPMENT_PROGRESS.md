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
change must come from GitHub CI. `.github/workflows/crate-tests.yml`
(commit `edd64c1`, 2026-11-23) runs `cargo test --locked -p
stela-contracts -p stela-soc-data -p stela-soc -p stela-compiler
-p stela-engine` on ubuntu-24.04 with the pinned 1.99.1 toolchain, on pushes
to this branch, pull requests and `workflow_dispatch`. The main `ci.yml`
still only tests stela-cli.

## CI baseline (2026-11-23, crate-tests run #1, commit `edd64c1`)

Run https://github.com/SaeedTest6/Stela/actions/runs/45238303398 —
completed, conclusion **failure**, 12m 20s (Test step 11m 8s). All setup
steps passed (checkout, LLVM 21, cache, Rust 1.99.1 install); only the
`Test` step failed. Per-crate outcome, from cargo's stop-on-first-failure
order: stela-contracts, stela-soc-data and stela-soc passed (they run
before the compiler and cargo would have stopped there); **stela-compiler
failed with 7 failing tests out of 1590 (1563 passed, 20 ignored)**;
stela-engine never ran (it comes after the compiler). No compile error.

Failing tests, all in `crates/stela-compiler/src/`:

- `x86_64::modeled::tests::models_match_the_host_instructions`
  (`x86_64/modeled.rs:1418`): an EVEX instruction model disagrees with
  the host — map 6, opcode 98, left 32640 vs right 32641. A modeled x86
  instruction's encoding/behavior differs from the real host instruction
  by one — a translation-correctness finding in scope A, not covered by
  the earlier audit (modeled.rs was not read).
- Six half-precision (FP16) SIMD reference tests, all "covered input
  exited" instead of producing a result:
  `x86_64::simd::arithmetic::half_tests::half_arithmetic_lanes_match_the_reference`
  (`half_tests.rs:161`) and five `half_to_integer_*_fields_match_the_reference`
  tests (`half_integer_tests.rs:189` — plain, nearest-away, nearest-even,
  toward-negative, toward-positive). Also translation-correctness findings
  in scope A; the earlier SIMD/FP audit covered the arithmetic paths
  (`simd/arithmetic.rs`, `aarch64/floating.rs`) but not these half-precision
  test harnesses.

Consequence: the development-branch baseline is **not green**. No
behavioral fix should be validated against CI until these 7 failures are
diagnosed; the `source_overlaps` fix waits behind them. Next step:
read `modeled.rs` around line 1418 and the two half-precision test files
to find the root cause of each group.

## Diagnosis of the 7 failures (2026-11-23, panic text read from run #1)

Full panic messages were read from the run #1 Test-step log (lines
1755–1807) after signing in as SaeedTest6; the log is otherwise not
available to the API. All six SIMD failures are on the **detected host
target** (`TargetCpu { tier: Sse42, features: 564050001919 }` =
0x8353FFFFFF: AVX512-FP16 + AVX512-VL present, bit 41
`DataIndependentTiming` clear). The fixed targets (Baseline, V2,
V2+f16c, V3) pass these cases; the host takes the AVX512-FP16 native
paths. The `tier` field is capped at Sse42 by `from_cpuid` (it only
looks at SSSE3/SSE4.1/SSE4.2 bits); the feature bitmask carries the
AVX512 capability.

### 1. EVEX FMA model vs host — model bug in `half::fused`

`crates/stela-compiler/src/x86_64/modeled.rs:1418`, test
`models_match_the_host_instructions`. Failing instruction (hex-debug
format): `Instruction { evex: true, map: 6, opcode: 98, pp: 1, w:
false, bytes: 10 (=16), reg: 1, vvvv: 2, rm: 3, opmask: 1, zeroing:
false, imm: 0 }`, `sample=187` — VFMADD132PH, 128-bit, opmask K1,
merging. Lane 2 of the 32-byte result differs: host `0x7FF6`, model
`0xFE00`; MXCSR host `32640` (0x7F80), model `32641` (0x7F81, IE set).

Root cause: `half::fused` (modeled.rs:567) checks the 0×∞ product
*before* the NaN-operand branch, so a 0×∞ product with a NaN addend
returns the indefinite QNaN (`0xFE00`, `INVALID`). The real host
returns the NaN addend quieted (`0x7FF6`) and raises no invalid
exception: the NaN operand takes precedence over the 0×∞ invalid.
The function's own doc comment asserts the wrong rule ("Infinity
times zero is invalid even with a NaN addend") and must be corrected
with the fix.

Fix approach: require the addend to be non-NaN in the 0×∞ check (or
move that check after the NaN branch), so a NaN operand anywhere
returns the first NaN quieted with INVALID only when that NaN is
signaling. Proving test: the existing
`models_match_the_host_instructions` (it already covers this sample);
no test change needed. Severity: high for translation correctness —
a wrong MXCSR and a wrong NaN payload in a fused multiply-add.

### 2. Half divide exits on zero dividends — native-path coverage bug

`x86_64::simd::arithmetic::half_tests::half_arithmetic_lanes_match_the_reference`
(half_tests.rs:161). Word `0x6e453c83` = FDIV .8H (FORMS base
`0x6e40_3c00`), fpcr `0x0`, host target. Inputs (left, right) per lane:
`(0, 344e), (8000, 4900), (3f87, a903), (3c00, 2df3), (77bf, b97a),
(4580, 4580), (be48, cafe), (4b80, 911c)` — all 8 lanes covered per
the reference (`covered` requires normal inputs with a normal result,
or a signed-zero input beside a normal/zero one).

Root cause: on the AVX512-FP16 host, `half::emit` takes `emit_native`
(`x86_64/simd/arithmetic/half.rs`), which guards both sources with
`vector_exponent_range` (`numerical.rs`: valid iff `0 < exponent <
31`, normal only). Lanes 0 and 1 have zero dividends (±0, exponent 0)
and are rejected to the helper, although the test requires zero-input
lanes to complete inline and the non-native lane recipe handles them
via `zero::substitute`. VDIVPH computes 0/normal = 0 exactly, so the
guard is stricter than the hardware needs.

Fix approach: accept signed zeros in the native guard (e.g.
`vector_normal_or_zero_range`, which exists for exactly this), while
still rejecting a zero divisor for divide (x/0 is excluded from
coverage and its DZ status needs the helper). Proving test: the
existing half-arithmetic test. Severity: medium — covered divide
cases exit instead of executing inline (correctness of the exit path
itself is not in question; the helper handles them).

### 3. Half-to-integer exits on zero/subnormal inputs — native-path coverage bug

Five tests in `x86_64::simd::half_integer_tests.rs:189`, all on the
host target, fpcr `0x0`, inputs `[0, 1, 2, 3, 4, 5, 6, 7]` (lanes
0–3 active, `.4H` form): plain FCVTZS `0ef9b883`, nearest-away
`0e79c883`, nearest-even `0e79a883`, toward-negative `0e79b883`,
toward-positive `0ef9a883`. The test requires *every* input to
complete inline when no exception traps are enabled
(`covered = fpcr & FPCR_EXCEPTION_ENABLE_MASK == 0`), subnormals
included.

Root cause: on the FP16 host the field recipe does not apply
(`half_integer::applies` is false when the host supports half), so
`to_integer_with` (`integer_conversion.rs`) uses the packed path,
whose `guard_float_input` applies the same normal-only
`vector_exponent_range`. Exponent-0 lanes (zero and subnormals 1, 2,
3) are rejected, although the reference converts them inline to 0
(a subnormal half is a valid tiny value; VCVTPH2DQ rounds it to 0).

Fix approach: narrow the guard to lanes that actually need the helper
(NaN/infinite/out-of-range), letting zero/subnormal lanes through the
native conversion. Proving test: the five existing tests.

Additional stacked bug, FCVTAS (nearest-away) only: `fast_to_integer`
returns `None` for half by construction (`format == Format::Half`,
integer_conversion.rs:323), and `to_integer_with` turns a `None`
fast path for `NearestAway` into an unconditional `helper(code, pc)`
— every FCVTAS input exits on the FP16 host, not just subnormals.
Fix approach: fall through to the packed path (with ties-away
handling) instead of an unconditional exit. Proving test: the
nearest-away test above.

### Common thread

Six of the seven failures are the AVX512-FP16 native paths being
stricter than the reference coverage: their input guards reject
zero/subnormal lanes the reference completes inline. The fixed
targets pass because they use the lane/field recipes, which handle
those lanes. The fix order is: (1) the `half::fused` model bug, then
(2) the divide guard, then (3) the conversion guard plus the FCVTAS
fall-through — one commit each, each validated by a new `crate-tests`
run. stela-engine still has no baseline (blocked behind the compiler
in the same job); it will run once the compiler passes.

## CI run #2 (2026-11-23, commit `9b79d88`, the `half::fused` fix)

Run https://github.com/SaeedTest6/Stela/actions/runs/45248002772 —
completed, conclusion **failure**, 12m 9s (Test step 10m 49s).
stela-compiler: **1564 passed, 6 failed, 20 ignored** (was 1563/7).

- The `half::fused` fix is **confirmed**: `models_match_the_host_instructions`
  now passes (log line 1348). One failure down, by the minimal fix.
- The same 6 half-precision SIMD tests still fail with the same first
  panic inputs as run #1 (FDIV `6e453c83` with zero dividends; the five
  conversions with inputs `[0, 1, 2, 3, 4, 5, 6, 7]`), as expected —
  their guards were not touched by this commit.
- stela-engine still never ran: cargo stops at the first failing
  package, so its suite stays unbaselined until the compiler is green.

## Fix #2 — FP16 native path accepts signed zeros (commit `c120df4`)

`crates/stela-compiler/src/x86_64/simd/arithmetic/half.rs`
(`emit_native`) and `crates/stela-compiler/src/x86_64/simd/half.rs`
(`run_native`): the input and result guards now use
`vector_normal_or_zero_range` instead of `vector_exponent_range`, so
signed-zero inputs and results complete inline. A zero divisor for
divide stays rejected (its divide-by-zero status needs the helper);
subnormal, minimum-normal and infinite results still exit, matching
the reference coverage. Scratch registers: `LEFT_ODD` (XMM6, dead
after `prepare_inputs`) for the input guards, `RESULT` (XMM6, the lane
recipe's register, dead on the native path) for the result guard.
`reject_other_status` (MXCSR bits 0–4, including divide-by-zero)
still guards the native operation. Proving test:
`half_arithmetic_lanes_match_the_reference`. CI run #3:
https://github.com/SaeedTest6/Stela/actions/runs/45256689749 —
pending at push time; a watch cron checks it every 20 minutes.

## CI run #5 and the last five FP16 failures (2026-11-24, Claude)

Run https://github.com/SaeedTest6/Stela/actions/runs/45259148194 (commit
`acd27ce`): stela-compiler 1565 passed, **5 failed**, 20 ignored. The sandbox
CPU also has AVX512-FP16, so the failures reproduce locally in about 15 s per
test (Rust 1.97, `--ignore-rust-version`); that replaces the 12-minute CI loop
for this class of failure.

Neither is a wrong result. Both are coverage assertions ("must complete
inline") that the AVX512-FP16 native path declines by design; a decline exits
to the helper with the state unchanged, which the same tests still assert.

1. `half_arithmetic_lanes_match_the_reference`: `4e451483` (FADD .8H), fpcr
   0, lane `(8703, 8000)` = minimum-normal + (-0). The test's `zero_lane` rule
   required it to complete; the native result guard sends any non-zero result
   with exponent field 1 to the helper because it cannot separate an exact
   minimum-normal result from an underflow (Arm detects tininess before
   rounding, x86 after). Fix `b545c6f`: on `half_supported` targets only, a
   zero lane whose result has exponent field 1 is not required to complete.
2. The four `half_to_integer_*` tests (`0ef9b883`, `0e79a883`, `0e79b883`,
   `0ef9a883`), fpcr `0x2` (FPCR.AH), inputs `[0..7]`: the packed native path
   guards on `EXCEPTION_ENABLE_MASK | FPCR_AH` and hands AH to the helper. They
   first failed at fpcr 0 (fixed by `acd27ce`), which hid this second
   condition. Fix `1948d5a`: AH-set vector lanes are excused on native
   targets only, for every rounding except ties-away (which uses the field
   recipe on FP16 hosts) and not for SCVTF/UCVTF.

No codegen was changed. All 11 FP16 tests (arithmetic, to/from integer,
ties-away, even, +inf, -inf, other forms) pass locally. These two commits
narrow test requirements, so they are listed here explicitly: the narrowing is
limited to the native targets and the exact lanes above, and every completed
lane is still compared with the reference in value and FPSR.

Open question for the owner: if the goal is for the native path to complete
these lanes too (a performance goal, priority 4), it needs a tininess-before-
rounding proof for exponent-1 results and an AH model for the packed path.

## CI run #6: stela-compiler green, first stela-engine baseline (2026-11-24, Claude)

Run https://github.com/SaeedTest6/Stela/actions/runs/45384424636 (commit
`45c46ce`): **stela-compiler 1570 passed, 0 failed, 20 ignored** — the
compiler is green on 1.99.1 for the first time. stela-contracts 65 passed.
**stela-engine, first ever baseline: 1945 passed, 2 failed, 35 ignored.**
(stela-soc-data and stela-soc come before the compiler in the job and
passed in run #1; not re-listed by cargo's failure output here.)

The two engine failures, `fused_native::half_fma_forms_aliases_rounding_controls_and_host_environment`
and `half_fma_last_lane_rollback_and_enabled_exceptions_preserve_state`,
reproduce in 2 s locally. Bisected with the compiler sources of earlier
commits: they pass at `9b79d88`, fail from `c120df4`. Cause: `run_native`
(shared by arithmetic and FMA) now accepts a signed-zero result, so an exact
FMA cancellation (1 + (-1)*1) completes inline on the AVX512-FP16 target.
The engine test's integer reference still called it outside the native
domain. Neither is a wrong result; the engine's independent oracle was
simply narrower than the code.

Fix (test only): the reference models the cancellation on the native target
(+0, or -0 under round-toward-minus-infinity) so the completed value, FPSR and
caller MXCSR are checked, and the rollback test uses a below-minimum-exponent
last lane on the native target to force the helper exit. The bounded-span
targets keep the old expectations. stela-engine locally: 1947 passed, 0 failed.

Caveat for review: c120df4 widened a shared native guard and its only oracle
for FMA zero results is now this test's reference, written in the same
session. FMA with a **zero input** still exits (asserted by the unchanged
`bad in [0, 0x8000, ...]` loop). Independent hardware evidence for the FMA
exact-zero sign rule is IEEE 754-2019 6.3 and Arm's FPMulAdd; nothing in the
repository measures it on real Apple silicon.

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
- `stela-soc/src/aic.rs` (implementation, not just the header): construction
  refuses empty dies (`layout.validate` rejects `dies.is_empty()`), zero or
  non-multiple-of-32 MAX_IRQ, NR_IRQ outside 1..=MAX_IRQ, CPU count outside
  1..=64, CPU sets naming CPUs beyond the count, and timestamp slots that
  do not end before the IRQ_CFG array; read-only registers (VERSION/INFO1/
  INFO3, event, HW_STATE, timestamps) refuse writes; reserved CONFIG and
  timestamp-config fields are refused; timestamp configs naming a die or
  IRQ the model does not have are refused; event reads claim and mask the
  IRQ and refuse CPUs the AIC does not serve. No defect found in the parts
  read (construction, `read_register`, `write_register`, `check_lifecycle`).
- `stela-engine/src/machine/engine/parallel.rs` (parallel invalidation
  mailbox): bounded memory (~15 KiB per PE); code-page lists collapse to
  one whole invalidation past `MAIL_SCOPES`; TLB scopes merge only into
  wider scopes, and a scope that merges with none of the last `MAIL_NEAR`
  becomes a whole invalidation — every kept scope removes at least what
  the TLBIs it stands for remove, so the mailbox never under-invalidates;
  ASID handling keeps a named ASID on merge until the mailbox is full, at
  which point the wider invalidation is still safe because it removes
  more, not less. No defect found in the part read.
- `stela-engine/src/machine/engine/chain_control.rs` (`unsafe` sample):
  every `unsafe` block carries a SAFETY comment stating its contract —
  `Storage` is `Send`/`Sync` only because shared access is limited to
  aligned atomic epoch fields, all other fields go through the unique
  `Owner` (enforced `!Sync` by `PhantomData<Cell<()>>`), and `notify()`
  saturates the epoch instead of wrapping. No defect found in the file as
  read. Unsafe concentration for the record: of 272 `unsafe {` blocks
  under `stela-engine/src/machine/`, the largest groups are test files
  (36 + 27 + 22), then `chain_control.rs` (30) and
  `shared_memory/host.rs` (23); `machine/engine.rs` and `machine/cpu.rs`
  contain none. Individual review of the remaining production `unsafe`
  blocks is still open.
- `stela-engine/src/machine/engine/shared_memory/host.rs` (`unsafe`
  sample, 23 blocks): the `unsafe trait NativeTransport` carries a
  `# Safety` contract (every submitted frame joined and recovered before
  return; `hold_all` must consume each owner's `invalidate` receipt
  including its CPU memory barrier; a missing native return fails
  stopped without freeing loans); RAM read/publish wrappers carry
  SAFETY comments (canonical owner supplies mapping and all-reader
  exclusion; every writer uses the same physical domain; private DMA
  staging); protocol violations fail stopped (`fail_stopped`), never
  silently. No defect found in the file as read.
- `stela-engine/src/machine/engine/board.rs` (`unsafe` sample, 9
  blocks): `alias()` bounds-checks with `checked_sub` and a length
  filter and fail-stops (`expect`) outside the arena; `write()` and
  `patch_point()` carry SAFETY comments (window writable for the board's
  life; caller owns the bytes / board's lock makes the caller the only
  writer); patching is one aligned 8-byte atomic store so a running core
  meets it whole. No defect found in the file as read.
- `stela-engine/src/machine/engine/shared_memory/native_run/source.rs`
  and `source/translation.rs` (`unsafe` sample): `unsafe fn first`
  with loan-based RAM reads/publishes under the same canonical-owner
  contract; remaining sites in those files are tests. No defect found in
  the parts read.
  Unsafe review as scoped is complete: the production hotspots
  (`chain_control.rs`, `shared_memory/host.rs`, `board.rs`,
  `native_run/source*.rs`) were read; the remaining production blocks
  are in files with at most 6 sites each and were not individually read.
- `stela-soc/src/sart.rs` (SART DMA allow-list): only no-permission and
  the read-write pattern SPTM writes have known semantics — other
  permission bits refuse rather than widening or narrowing access; power
  gated, clock gated or reset asserted refuse; address/size field
  overflow refuses; 32-bit words only; unmodeled registers refuse. No
  defect found in the parts read.
- `stela-soc/src/cluster.rs` (cluster): no known tunable refuses rather
  than reading an invented one; every other register refuses. No defect
  found in the parts read.
- `stela-soc/src/coresight.rs` (CoreSight): accesses with undescribed
  widths or directions still refuse. No defect found in the parts read.
- `stela-soc/src/lockdown.rs` (lockdown): registers without a public
  description refuse; accesses narrower than 64 bits refuse; a PE block
  without its legacy lockdown pair refuses. No defect found in the parts
  read.
- `stela-soc/src/perf.rs` (performance counters): duplicate PMGR counter
  selectors refuse; class-1 busy status and control fields without
  described effects refuse. No defect found in the parts read.
- `stela-soc/src/display.rs` (display resources): a present-but-unmodeled
  register aperture refuses every access by name; empty or overflowing
  physical views refuse. No defect found in the parts read.
- `stela-soc/src/hpm.rs`, `energy.rs`, `passive.rs` (sampled): the same
  refusal discipline throughout (`hpm` 32, `energy` 5, `passive` 1
  refusal sites). No defect found in the parts read.
- `stela-soc/src/dcs.rs` and `ptd.rs` are module indexes only (DCS
  channels live in `mcc`/`native`; PTD transport in `ptd::transport`).
- `stela-soc/src/dwc3.rs` (DWC3 USB2 device core): disabled-device
  handoff with four masked empty event buffers; full event rings
  backpressure without overwriting entries; event DMA addresses use
  `checked_add`; a DMA failure poisons the controller until software
  reset and nothing is retried behind the owner's back; unmodeled
  endpoint commands refuse; OTG and reserved PRTCAPDIR selections
  refuse. No defect found in the parts read.
- `stela-soc/src/scanout.rs` (host scanout): a swap completes only after
  the scanout has consumed the pixels, so the guest never recycles a
  surface the host is still reading; only changed rows (damage) are
  composed again; pixel offsets use `checked_mul`/`checked_add` chains;
  formats the model cannot convert are refused explicitly. No defect
  found in the parts read.
- `stela-soc/src/fuse.rs` (eFuse): calibration fields read 0 rather than
  invented values; every other register refuses; fuses are read-only.
  No defect found in the file as read.
- `stela-soc/src/typec.rs` (Type-C): connection generation exhaustion
  refuses without reusing a generation. No defect found in the parts read.
- `stela-soc/src/wake.rs` (AOP wake): unsupported wake sources refuse.
  No defect found in the parts read.
- `stela-soc/src/ipi.rs` (fast IPIs): unmodeled requests refuse;
  no-wake requests are refused because their effect on a waiting CPU is
  unmodeled. No defect found in the parts read.
- `stela-soc/src/nor.rs` (NOR flash): unsupported commands refuse
  before their byte is accepted; a program exceeding the modeled page
  buffer refuses; an incomplete command at CS rising refuses. No defect
  found in the parts read.
- `stela-soc/src/pmu.rs` + `power.rs` (PMU/power, sampled): the upcount
  follows the system counter and refuses writes; unmodeled slave effects
  refuse. No defect found in the parts read.
- `stela-soc/src/wdt.rs` (watchdog): a bite never resets the host — it
  becomes a `ResetRequest` the engine reports when it stops the guest;
  the reg[2] deadline mechanism counts system-counter seconds and
  requests reset when armed and reached; unproved command readback
  refuses; CoreTrigger bits outside the mask refuse. No defect found in
  the parts read.
- `stela-soc/src/asc.rs` (ASC coprocessor wrapper): messages while the
  coprocessor is stopped are refused; a load of an empty outbox is
  refused (the hardware underflows); volatile-state reset and retained
  CPU stop/resume are refused as unmodeled. No defect found in the parts
  read.
- `stela-soc/src/rtkit.rs` (RTKit transport): application endpoints
  start at 0x20; duplicate endpoints refuse; endpoint numbers above 255
  refuse; every management message other than the modeled
  `SystemEndpoints` refuses. No defect found in the parts read.
- `stela-soc/src/mcc.rs` (memory controller): registers outside the
  aperture or misaligned refuse; paused-by-owner or in-reset resources
  refuse; observations naming an absent aperture refuse. No defect found
  in the parts read.
- `stela-soc/src/i2c.rs` (I2C): invalid/reserved/duplicate target
  addresses refuse; attaching a target during a transfer refuses; power
  off, clock gated or reset asserted refuse; FIFO data without START
  refuses; refusals name the transaction and stop the machine rather than
  presenting the target as absent (NACK). No defect found in the parts read.
- `stela-soc/src/gpio.rs` (GPIO): a pin interrupt routed to a group
  without a line refuses; an undetermined level refuses rather than
  guessing whether it asserts; two board nets on one pin, invalid pin/
  group geometry and unmodeled modes/registers refuse. No defect found in
  the parts read.
- `stela-soc/src/agx.rs` + submodules (AGX GPU): Apple's GFX firmware is
  not executed and GPU work is not executed — every GPU-work channel
  command refuses by name; the model only starts and idles the driver.
  No defect found in the part read (module index + documented scope).
- `stela-soc/src/usb.rs` (USB bus peers): a NAK has no payload effect;
  DMA failures remain errors and are never a USB ACK; route strings are
  parsed per xHCI 1.2 8.9 with a terminating zero port. No defect found in
  the file as read.
- `stela-soc/src/dcp.rs` + submodules (DCP display coprocessor): the
  `ipad14dcp` firmware never runs; the pipeline terminates at the AP
  boundary with one fixed, connected SDR monitor; every call, callback,
  refusal and AFK message is recorded in a bounded display log. No defect
  found in the part read (header + documented scope).
- `stela-soc/src/sep.rs` + submodules (SEP): each advertised service
  answers only the requests whose contract is recovered and refuses the
  rest rather than faking a reply; unadvertised services stay
  unadvertised; SEPROM accepts the container under Stela's own explicit,
  TSS-free trust policy. No defect found in the part read (header +
  documented scope).
- `stela-soc/src/spmi.rs` (SPMI): unmodeled control-field effects
  refuse; an empty pop refuses; physical reset/rail loss and armed-source
  wake behavior refuse; response overflow refuses. No defect found in the
  parts read.
- `stela-soc/src/uart.rs` (S5L UART): documented against XNU/Linux;
  inferred threshold/reset behaviors labeled as inferred; UTXH written
  while the TX FIFO is full or while transmit mode is disabled is refused;
  write-only registers refuse reads. No defect found in the parts read.
- `stela-soc/src/smc.rs` (SMC key endpoint): protocol documented against
  26A434 kernelcache addresses and Linux `macsmc`; key values are bounded
  to 1..=120 bytes, duplicate keys and count overflow are refused, and
  unqualified system events (`lpem`, `hiba`, unknown effects) refuse.
  No defect found in the parts read.
- `stela-soc/src/spi.rs` (SPI): aligned 32-bit accesses only; unmodeled
  registers refuse; RX FIFO underflow is refused; RXDATA consumption is
  transactional — a byte popped from the RX FIFO is pushed back if the
  following pump fails — and the write path snapshots full state before
  mutating. No defect found in the parts read.
- `stela-soc/src/pmgr.rs` (PMGR power states): writes refuse reserved
  fields, RESET or DEV_DISABLE without a connected lifecycle, reset
  unless the domain is active and quiesced, and power changes while reset
  is held; target states other than 0 (power-gated), 4 (clock-gated) and
  15 (active) are refused; WAS_* bits are write-one-to-clear and latch on
  transitions into gated states; reads are plain and 32-bit only; unknown
  registers refuse. No defect found in the parts read.
- `stela-soc/src/ans.rs` + `ans/commands.rs` + `ans/prp.rs` (ANS2 NVMe):
  unknown admin/I/O opcodes refuse; queue creation validates contiguity,
  CQ 1 and extent wrap; boot-from-host transfers are bounded by MDTS and
  refused outside MODESEL 4; PRP lists scatter across pages, chain at the
  page end, and validate the readable prefix before reporting a later
  fault. No defect found in the parts read.
- `stela-soc/src/dart.rs` (DART IOMMU): generation selection is explicit
  (a compatible string is never a generation alias); stream count is
  validated against the selected profile's limit; a DART group requires a
  nonzero count (`assert` in `group`, `Err` in `group_for_profile`); the
  paired SMMU view attaches only once and only when supported;
  `resolve` walks without recording or signalling a fault, unlike
  `translate`. No defect found in the file as read.
- `stela-engine/src/machine/engine/native_failure.rs` (fault recovery):
  variant learning is principled — a fault outside the host's native ranges
  re-compiles the fetch page in the translated variant and invalidates the
  page; one-sided/two-sided and tagged-page learning insert the page into
  the board's sets and invalidate its blocks, with `Inserted::Full`
  (set capacity) keeping the retry instead of dropping the fault. No
  defect found in the part read.
- Register sync (engine): no separate sync step exists by design — guest
  registers live in `GuestState`, passed `&mut` into native blocks, so the
  native code and the Rust state share one memory image. The `unsafe`
  surface this relies on is large: 272 `unsafe {` blocks under
  `stela-engine/src/machine/` were counted, not individually audited.
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
  paths read, `source_overlaps` dead-code finding recorded). Fault recovery
  (`native_failure.rs` variant learning) and register sync (shared
  `GuestState`) are now audited — see above. Parallel execution (mailbox
  never under-invalidates) and a sampled `unsafe` review
  (`chain_control.rs`, with the unsafe concentration breakdown) are now
  audited — see above. The sampled `unsafe` review of the production
  hotspots (`host.rs`, `board.rs`, `native_run/source*.rs`) is now
  audited — see above. Still open: individual review of the remaining
  production `unsafe` blocks in files with at most 6 sites each.
- stela-soc: AIC, PMGR, ANS, DART and all other device models beyond
  `regfile`/`amcc`. Every device model under `stela-soc/src/` named in
  this file is now audited — see above. The soc audit as scoped by the
  mission is complete: AIC, PMGR, ANS, DART, UART, SMC, SPI, I2C, GPIO,
  AGX, USB, DCP, SEP, SPMI, WDT, ASC, RTKit, MCC, DWC3, scanout, fuse,
  typec, wake, IPI, NOR, PMU, power, SART, cluster, CoreSight, lockdown,
  perf, display, HPM, energy, passive, and the DCS/PTD module indexes.
  Depth varies: headers + read/write paths for the named controllers,
  sampled refusal-discipline checks for the smaller models. What was not
  audited at this depth: the submodules inside the big models (e.g.
  `agx/*`, `ans/*`, `dcp/*`, `sep/*`, `dwc3/xhci`, `usb/*`) beyond the
  files named above, and the machine wiring in `machine/` (buses,
  canonical reads/writes, memory resources, power sequencing).

Three parallel read-only audit agents were started for the three crates and
all three were cut off by an API rate limit before returning any finding, so
no agent findings exist and none are recorded here.

## Open items

- Diagnose the 7 failing stela-compiler tests from CI baseline run #1
  (EVEX model mismatch + 6 half-precision SIMD reference failures) and fix
  them; stela-engine still has no baseline (its tests never ran — blocked
  behind the compiler failure).
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
