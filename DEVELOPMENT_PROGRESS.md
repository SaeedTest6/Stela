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
  audited — see above. Still open: individual review of the remaining
  production `unsafe` blocks under `machine/` (notably
  `shared_memory/host.rs`).
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
