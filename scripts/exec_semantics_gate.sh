#!/usr/bin/env bash
# exec_semantics_gate.sh — run the test tiers CI could not reach, and PROVE they ran.
#
# WHY THIS EXISTS
# ---------------
# Measured at f2a2d87d: 123 integration-test files / 262 test functions were reachable
# by NO `cargo test` invocation in .github/workflows/ci.yml or scripts/preflight.sh.
# Every broad `cargo test` in CI runs `--no-default-features` with at most
# {std-surface, cross-module-imports, autodiff, mlir-lowering, cpu-buffers}; the only
# runs that enable `mlir-build` are `--test <single-target>` SELECTORS, and exactly ONE
# of the 110 mlir-build-gated files (cross_substrate_identity) is named by one.
#
# A file-level `#![cfg(all(unix, feature = "mlir-build", ...))]` that is unsatisfied
# ERASES the file — and the harness still prints `ok. 0 passed` and exits 0:
#
#   $ cargo test --no-default-features --features std-surface,cross-module-imports \
#       --test alias_miscompile_run --test array_oob_trap_run
#   running 0 tests
#   test result: ok. 0 passed; 0 failed; ...      <- the alias-miscompile gate
#   running 0 tests
#   test result: ok. 0 passed; 0 failed; ...      <- the array-OOB bounds-TRAP gate
#   CARGO EXIT=0
#
# So the alias-miscompile gate and the ARRAY_OOB_CONTRACT=DETERMINISTIC_BOUNDS_TRAP
# gate were EMPTY HARNESSES in every CI run. A lowering regression that made `a[i]`
# skip its bounds check, or reintroduced the alias miscompile, was caught by NOTHING.
#
# GATE-EVIDENCE RULE (the doctrine scripts/preflight.sh already states in its header):
# never accept `exit 0` as proof a gate ran. Every tier below asserts a POSITIVE test
# count against a floor, so a future feature-gating accident cannot silently zero it.
#
# The floor counts tests EXECUTED (passed + failed + ignored). That is deterministic
# from the source tree given the feature set — unlike a pass count it does not move
# with the environment — and it collapses to 0 under exactly the accident being
# guarded against.
#
# "DID IT RUN" IS HALF THE QUESTION; "DID IT PASS" IS THE OTHER HALF.
# The tier verdict rests on THREE independent readings of the same run, because any
# one of them can be defeated alone:
#   1. the triage below, over cargo's rerun hint — for EVERY harness kind it names
#      (`--test <n>`, `--lib`, `--doc`, `--bin <n>`, `--bench <n>`), not just the
#      integration targets; a hint this gate cannot parse is itself a failure;
#   2. the aggregate `failed` count summed from the harnesses' own `test result:`
#      lines, attributed per-harness so the quarantine keeps working;
#   3. cargo's own exit status, recorded into the log and compared — a non-zero exit
#      nothing in the log can account for fails the tier.
# Measured before (2) and (3) existed and while (1) matched only `--test <n>`: a tier
# log carrying a lib harness with `3 failed` printed `failed=3` and, three lines
# later, `ok[exec]: ... 0 failing` — exit 0. scripts/test_exec_semantics_gate.py
# replays that log and eight more, and asserts the exit code of each.
#
# Usage:
#   scripts/exec_semantics_gate.sh                 # all tiers
#   scripts/exec_semantics_gate.sh exec            # one tier: exec | lowering | pkg
#   scripts/exec_semantics_gate.sh --print-count   # report counts, never fail
#   scripts/exec_semantics_gate.sh --from-log <log> <tier>
#                                                  # ANALYSIS-ONLY: re-read a saved
#                                                  # tier log without running cargo
set -uo pipefail
cd "$(dirname "$0")/.."

# ---------------------------------------------------------------------------
# TIER DEFINITIONS.  RE-MEASURED at the wave that added the fail-closed capability
# gates (`scripts/exec_semantics_gate.sh --print-count`, one run per tier).
# Floors sit a hair under the measured value: a feature-gating accident erases whole
# FILES (dozens to hundreds of tests at once), so a ~0.8% margin costs the gate
# nothing while keeping a one-test environmental difference from redding main.
# Re-derive after intentionally adding or removing tests: --print-count.
#
# A floor is a RATCHET, not a historical note. Left at its first-measured value it
# accrues slack — and slack is deletable test coverage: at 1900 the exec tier carried
# 202 tests of headroom, enough to delete EVERY test in the largest gate file in the
# tree and still print `ok[exec]`. Re-measure at landing, every landing.
#
#   tier      features                                   harnesses  executed  floor
#   exec      mlir-build std-surface cross-module-imports      332      2102   2085
#   lowering  std-surface,mlir-lowering                        327      1830   1815
#   pkg       pkg                                              326      1415   1400
#
# (Superseded: measured@f2a2d87d was 317/1913, 318/1663, 317/1265.)
# ---------------------------------------------------------------------------
TIERS=(exec lowering pkg)

# `exec` — the executable-semantics tier. 110 mlir-build-gated files, including the
# alias-miscompile gate, the array-OOB bounds-trap gate and the array bounds/dtype
# gate. Dropping ANY of the three features silently erases most of it.
FEATURES_exec="mlir-build std-surface cross-module-imports"
FLOOR_TESTS_exec=2085
FLOOR_HARNESSES_exec=328
# MIND_BENCH_REQUIRE=1 turns "MLIR toolchain missing -> skip" into a hard failure, so
# this tier cannot pass vacuously on a runner where mlir-opt/clang never installed.
# Correct ONLY here: this is the tier that actually enables mlir-build.
REQUIRE_TOOLCHAIN_exec=1

# `lowering` — 8 files / ~53 tests carry
# `#![cfg(all(feature = "std-surface", feature = "mlir-lowering"))]`. ci.yml runs each
# of those features ALONE (the 'Test (gated ...)' and 'Run gated tests ...' steps of
# the build_test job) and never together, so the whole group was erased in both runs.
FEATURES_lowering="std-surface,mlir-lowering"
FLOOR_TESTS_lowering=1815
FLOOR_HARNESSES_lowering=323
# NOT set here. These tiers deliberately build WITHOUT mlir-build, so a target that
# needs a cdylib emit (phase_g_keystone_bootstrap) correctly reports
#   error[build]: cdylib emit requires the 'mlir-build' feature
# and skips. Under MIND_BENCH_REQUIRE=1 that correct skip becomes a hard failure and
# the tier reds for a reason that is not a defect — measured while building this gate.
# The keystone is gated for real by the `exec` tier and by preflight's own 7/7 step.
REQUIRE_TOOLCHAIN_lowering=0

# `pkg` — tests/package_basic.rs + tests/package_traversal.rs are `#![cfg(feature =
# "pkg")]`. ci.yml's feature-compile matrix runs `cargo check --features pkg` but never
# `cargo test`, so neither had ever executed.
FEATURES_pkg="pkg"
FLOOR_TESTS_pkg=1400
FLOOR_HARNESSES_pkg=322
REQUIRE_TOOLCHAIN_pkg=0

# ---------------------------------------------------------------------------
# QUARANTINE — targets RED at f2a2d87d for a reason NAMED here.
#
# A RATCHET, not an excuse. Any failure in a target NOT listed fails the gate, and a
# listed target that starts PASSING also fails the gate (with an instruction to delete
# its line). The list may only ever shrink.
#
# Every entry is a genuine, separately-scoped defect that this tier was HIDING — they
# are the payload of the finding, not collateral from it.
# deferred: each needs its own fix; none is closed by this gate.
#
# An entry is the name the triage reports: an integration target's bare name, or one
# of the `@`-prefixed pseudo-targets (`@lib`, `@doc`, `@bin:<name>`, `@bench:<name>`)
# that stand for the harness kinds cargo does not name with `--test`. See
# rerun_selector() below.
#
#   std_surface_intrinsics       — `each_intrinsic_lowers_to_func_call_with_private_decl`
#       expects `func.call @__mind_load_i64(%`; the intrinsic now lowers inline to
#       `llvm.inttoptr` + `llvm.load`. Stale shape expectation, not a miscompile.
# CRITICAL_<tier> — per-harness minimums for the gates each tier NAMES as the reason
# it exists. An AGGREGATE floor cannot protect a SPECIFIC test: measured slack was 14
# tests against FLOOR_TESTS_exec, and an erased test file still prints
# `test result: ok. 0 passed` so the HARNESS count does not move either. Re-erasing
# exactly the two files this gate was written to defend therefore produced
# ALL TIERS OK / exit 0 — the gate could not protect the thing it names.
# Format: "<target> <min_executed>", checked INDEPENDENTLY of the floors.
#
# PER-TIER, not global. A global list is wrong twice over: a target is only expected
# to run in the tier whose features satisfy its `#![cfg(...)]`, so checking the
# mlir-build gates in the `lowering`/`pkg` tiers would red those tiers for a target
# they correctly do not build — and it would leave `lowering` and `pkg` with NO
# per-harness minimum at all, which is precisely the hole this check exists to close
# (pkg's 2 files / 2 tests are invisible inside an aggregate floor of 1255).
#
# The count compared is tests EXECUTED (passed + failed + ignored), not passed: this
# check answers "did the substance run", and a target that ran and FAILED is caught
# by the quarantine triage below with a message that names the real problem. Counting
# `passed` here would report a failing gate as "did not run".
#
# A row is added by the SAME change that adds the gate it protects: a new gate whose
# only protection is the aggregate floor is a gate anyone may delete. The two
# fail-closed skip gates below carry no file-level `cfg` and no `required-features`,
# so they build and run in ALL THREE tiers — measured per tier (21 and 6 executed in
# exec, lowering and pkg alike), never assumed from one of them.
CRITICAL_exec=(
  "alias_miscompile_run 1"    # the alias-miscompile regression gate
  "array_oob_trap_run 1"      # ARRAY_OOB_CONTRACT=DETERMINISTIC_BOUNDS_TRAP
  "fail_closed_capability_skip 20"  # measured 21; the capability-skip helper contract
  "fail_open_skip_site_ratchet 5"   # measured 6; the shrink-only fail-open backlog
)
# The two largest members of the std-surface+mlir-lowering group that no CI run
# enabled BOTH features for; 26 of the group's 57 tests live in these two files.
CRITICAL_lowering=(
  "extern_c_phase_a 1"
  "extern_c_phase_b 1"
  "fail_closed_capability_skip 20"
  "fail_open_skip_site_ratchet 5"
)
# The entire reason the `pkg` tier exists: ci.yml only ever `cargo check`ed pkg.
CRITICAL_pkg=(
  "package_basic 1"
  "package_traversal 1"
  "fail_closed_capability_skip 20"
  "fail_open_skip_site_ratchet 5"
)

QUARANTINE_exec=(
)
QUARANTINE_lowering=(std_surface_intrinsics)
QUARANTINE_pkg=()

# ENV_TOLERATED — may fail LOCALLY for a documented environmental reason and pass in
# CI's fresh checkout (or the reverse). Neither outcome fails the gate. Kept distinct
# from QUARANTINE so a real regression is never hidden behind a local-only excuse.
#
#   g2_differential_mlir      — dlopens the gitignored, possibly stale in-tree
#       libmindc_mind.so. scripts/preflight.sh already excludes it for this reason.
#   mindfuzz_cross_substrate  — needs the MLIR toolchain; soft-skips in ci.yml's
#       build_test job. Also already excluded by scripts/preflight.sh.
ENV_TOLERATED_exec=(g2_differential_mlir)
# g2_differential_mlir is listed for ALL THREE tiers, not just exec.
#
# It has no [[test]] required-features entry, so cargo builds it in every tier — but
# it dlopens a libmindc_mind.so that only the exec tier's feature set can produce
# (FEATURES_exec carries mlir-build; lowering and pkg deliberately do not). In those
# two tiers it therefore skips and emits `ran=0`, which the SKIP-MARKER CONSUMER above
# treats as fatal. Without these entries that consumer would RED two tiers that are
# green today, for a target that cannot meaningfully run in them.
#
# This is not a weakening: g2 is a real gate in exec, where it is now also exempt from
# crash-tolerance (a MIND_CRASH escapes tolerance and fails the tier). Here it records
# an honest environmental skip.
#
# The long-term fix this comment used to only RECOMMEND has since been APPLIED: both
# g2_differential_mlir and mindfuzz_cross_substrate now carry
# `required-features = ["mlir-build"]` in Cargo.toml, so cargo does not build them in
# tiers that cannot run them. Structural absence beats runtime tolerance.
#
# mindfuzz_cross_substrate is therefore no longer listed below for `lowering`/`pkg`:
# it is not built there at all, so tolerating its failure was dead configuration
# describing a target those tiers never see. g2 stays listed for `exec`, where it IS
# built and can fail for the documented environmental reason.
ENV_TOLERATED_lowering=(g2_differential_mlir)
ENV_TOLERATED_pkg=(g2_differential_mlir)

# ---------------------------------------------------------------------------
print_only=0
# --from-log: analyse a log this script already wrote (it keeps them at a stable
# path precisely so the "which test, and why" question can be answered after the
# fact) WITHOUT running cargo. It is analysis, never evidence: the pass banner is
# deliberately withheld in this mode, so a green CI line can never be produced by
# replaying a saved log.
from_log=""
want=()
while [ $# -gt 0 ]; do
  case "$1" in
    --print-count) print_only=1 ;;
    --from-log)
      shift
      from_log="${1:-}"
      [ -n "$from_log" ] || { echo "--from-log needs a log path" >&2; exit 2; } ;;
    exec|lowering|pkg) want+=("$1") ;;
    *) echo "usage: $0 [--print-count] [--from-log <log> <tier>] [exec|lowering|pkg ...]" >&2; exit 2 ;;
  esac
  shift
done
if [ -n "$from_log" ]; then
  [ -r "$from_log" ] || { echo "--from-log: cannot read $from_log" >&2; exit 2; }
  # The tier selects the floors, quarantine and ENV_TOLERATED list to judge the
  # log against, so it cannot be inferred from the file.
  [ ${#want[@]} -eq 1 ] || { echo "--from-log needs exactly one tier" >&2; exit 2; }
fi
[ ${#want[@]} -eq 0 ] && want=("${TIERS[@]}")

in_list() { local n=$1; shift; local e; for e in "$@"; do [ "$e" = "$n" ] && return 0; done; return 1; }

# cargo names four harness KINDS in its rerun hint, and only one of them is a
# `--test <name>` integration target. The other three (`--lib`, `--doc`,
# `--bin <n>`, `--bench <n>`) are given PSEUDO-TARGET names below so they can be
# triaged, reported and quarantined exactly like a real target. The `@` prefix
# cannot collide with a cargo target name (those are file stems: [A-Za-z0-9_-]),
# so `@lib` can never be confused with an integration test called `lib`.
#
#   @lib            src/lib.rs unit tests        rerun: --lib
#   @doc            documentation tests          rerun: --doc
#   @bin:<name>     a binary's unit tests        rerun: --bin <name>
#   @bench:<name>   a bench target's tests       rerun: --bench <name>
rerun_selector() {
  case "$1" in
    @lib)     echo "--lib" ;;
    @doc)     echo "--doc" ;;
    @bin:*)   echo "--bin ${1#@bin:}" ;;
    @bin)     echo "--bins" ;;
    @bench:*) echo "--bench ${1#@bench:}" ;;
    *)        echo "--test $1" ;;
  esac
}

overall=0
for tier in "${want[@]}"; do
  eval "features=\$FEATURES_$tier"
  eval "floor_t=\$FLOOR_TESTS_$tier"
  eval "floor_h=\$FLOOR_HARNESSES_$tier"
  eval "require_toolchain=\$REQUIRE_TOOLCHAIN_$tier"
  eval "quarantine=(\"\${QUARANTINE_$tier[@]}\")"
  eval "tolerated=(\"\${ENV_TOLERATED_$tier[@]}\")"
  eval "critical=(\${CRITICAL_$tier[@]+\"\${CRITICAL_$tier[@]}\"})"

  echo
  echo "=============================================================="
  echo "== tier '$tier':  cargo test --no-default-features --features \"$features\""
  echo "==   MIND_BENCH_REQUIRE=$require_toolchain (1 = a missing MLIR toolchain is a FAILURE, not a skip)"
  echo "=============================================================="
  # Keep the log at a STABLE path, not a mktemp that is deleted on the way out:
  # when this gate fails, the next question is always "which test, and why", and a
  # 40-line tail of a deleted file cannot answer it. CI uploads nothing, so the path
  # is printed on failure and the file is left in place for the developer.
  log="${MIND_TIER_LOG_DIR:-${TMPDIR:-/tmp}}/mind-tier-$tier.log"
  mkdir -p "$(dirname "$log")"

  if [ -n "$from_log" ]; then
    log="$from_log"
    # cargo's exit status is RECORDED IN THE LOG (see the marker written below), so
    # a replayed log is judged on the same evidence as the run that produced it.
    # Before that, this mode read "n/a" and the status assert below could not run at
    # all — the analysis silently answered a weaker question than the live gate.
    # A log written before the marker existed reports `unknown`: that leg of the
    # verdict is then honestly UNAVAILABLE rather than silently assumed to be 0.
    cargo_status=$(sed -n 's/^MIND_TIER_CARGO_EXIT=\([0-9]\{1,\}\)$/\1/p' "$log" | tail -1)
    cargo_status="${cargo_status:-unknown}"
    echo "ANALYSIS-ONLY: re-reading $log; NO tests were run by this invocation."
    echo "               recorded cargo exit: $cargo_status"
  elif [ "$require_toolchain" = 1 ]; then
    MIND_BENCH_REQUIRE=1 cargo test --no-default-features --features "$features" \
      --no-fail-fast >"$log" 2>&1
    cargo_status=$?
    echo "MIND_TIER_CARGO_EXIT=$cargo_status" >>"$log"
  else
    cargo test --no-default-features --features "$features" \
      --no-fail-fast >"$log" 2>&1
    cargo_status=$?
    echo "MIND_TIER_CARGO_EXIT=$cargo_status" >>"$log"
  fi

  # --- POSITIVE-COUNT ASSERT (the anti-silent-zeroing core) ----------------
  harnesses=$(grep -c '^test result:' "$log")
  read -r passed failed ignored <<<"$(
    grep '^test result:' "$log" | awk '{p+=$4; f+=$6; i+=$8} END {print p+0, f+0, i+0}'
  )"
  executed=$((passed + failed + ignored))

  echo "harnesses      : $harnesses  (floor $floor_h)"
  echo "tests executed : $executed  (floor $floor_t)  [passed=$passed failed=$failed ignored=$ignored]"

  if [ "$print_only" = 1 ]; then continue; fi

  rc=0
  # A test file that does not COMPILE also yields zero harnesses, and it is a very
  # different bug from a cfg-erased one — distinguish them, or the gate sends the
  # reader hunting a feature flag when the real problem is a broken test source.
  # (Hit for real while building this gate: an in-flight test file calling a
  # non-existent `TempDir::join` failed the whole build with harnesses=0.)
  mapfile -t build_errs < <(
    grep -E '^error(\[[A-Z0-9]+\])?: ' "$log" | grep -v '^error: test failed' | sort -u | head -5
  )
  # --- CRITICAL HARNESS MINIMUMS (independent of the aggregate floors) ------
  # Parsed per-target from the `Running ...` / `test result:` pairing, so erasing
  # one file is visible even when the tier total still clears its floor. Runs
  # BEFORE the floor chain and sets `overall` on its own, so the two verdicts are
  # independent rather than mutually exclusive.
  crit_bad=()
  for spec in ${critical[@]+"${critical[@]}"}; do
    ctarget="${spec%% *}"; cmin="${spec##* }"
    # Anchor on the exact `Running tests/<target>.rs` line cargo prints, not a
    # substring: `Running .*foo-` also matches a sibling target named
    # `bar_foo`, which would let an erased gate borrow another file's count.
    cran=$(awk -v t="$ctarget" '
      index($0, "Running tests/" t ".rs") { seen=1; next }
      seen && /^test result:/ {
        n=0
        for (i=1;i<=NF;i++)
          if ($i=="passed;" || $i=="failed;" || $i=="ignored;") n += $(i-1)
        print n; exit
      }' "$log")
    cran=${cran:-0}
    if [ "$cran" -lt "$cmin" ]; then
      crit_bad+=("$ctarget executed $cran test(s) (need >=$cmin)")
    fi
  done
  if [ ${#crit_bad[@]} -gt 0 ]; then
    overall=1
    echo "FAIL[$tier]: a CRITICAL harness did not run its tests."
    for b in "${crit_bad[@]}"; do echo "  $b"; done
    echo "  These are named in this script's header as the reason it exists. They are"
    echo "  checked per-harness precisely because an aggregate floor cannot protect a"
    echo "  specific test: an erased file still prints 'ok. 0 passed' and the tier"
    echo "  total can absorb it inside its slack."
  fi

  if [ ${#build_errs[@]} -gt 0 ] && [ "$harnesses" -lt "$floor_h" ]; then
    echo
    echo "FAIL[$tier]: the tier did not BUILD — this is a compile error, not a cfg problem."
    for e in "${build_errs[@]}"; do echo "  $e"; done
    echo "  full log: $log"
    rc=1
  elif [ "$executed" -lt "$floor_t" ] || [ "$harnesses" -lt "$floor_h" ]; then
    echo
    echo "FAIL[$tier]: the tier did not prove it ran."
    echo "  executed=$executed (need >=$floor_t), harnesses=$harnesses (need >=$floor_h)"
    echo "  A COLLAPSE here means test files were cfg'd OUT by a missing feature, NOT"
    echo "  that tests were deleted — cargo prints 'ok. 0 passed' and exits 0 for an"
    echo "  erased file. Check that --features \"$features\" is intact."
    rc=1
  fi

  # --- SKIP-MARKER CONSUMER: ran=0 is a failure to EXECUTE -----------------
  # A gate that cannot run prints an honest `SDLC-GATE <name> ran=<n> fail=<k>`
  # marker instead of asserting nothing in silence (tests/g2_differential_mlir.rs
  # prints one when the pure-MIND oracle .so is unobtainable). Until now NOTHING
  # read those lines — grep found producers and no consumer — so "compared every
  # fixture and found no divergence" and "compared nothing" produced the same
  # green tier. A marker nobody reads is documentation, not a gate.
  #
  # This answers a DIFFERENT question from the triage below. That one asks "did
  # it pass"; this one asks "did it run at all", and the two need separate
  # verdicts because the fixes are unrelated: a failure to pass is a code defect,
  # a failure to execute is a missing input or an erased harness.
  #
  # Environmental skips stay possible — through the list that ALREADY documents
  # them per-tier, rather than a second list that would drift out of step with
  # it. The default is fail-closed: a ran=0 marker from any target NOT named in
  # ENV_TOLERATED_<tier> fails the tier, so the next gate to print a marker is
  # enforced without anyone remembering to wire it up.
  #
  # Attribution is per-target (the enclosing `Running tests/<t>.rs` block), not
  # per-log, so one tolerated target cannot excuse another's silence.
  mapfile -t markers < <(
    awk '
      # Clear attribution at EVERY compilation-unit boundary, not just at
      # `Running tests/*.rs`. cargo also emits `Doc-tests <crate>`,
      # `Running benches/<x>.rs` and `Running unittests src/lib.rs`; without this
      # reset a marker printed from one of those inherits the identity of whichever
      # tests/<t>.rs block appeared last. Measured: a marker inside a `Doc-tests`
      # block was reported as coming from tests/filler_299.rs, which printed nothing.
      # Two wrong outcomes — the gate names the wrong file, and if that borrowed
      # carrier happens to be env-tolerated, a genuine ran=0 is silently downgraded
      # from fatal to tolerated. That is the exact false-green class this consumer
      # exists to remove, so it must fail closed to <unattributed> instead.
      /^[[:space:]]*(Running|Doc-tests) / { t = "" }
      match($0, /Running tests\/[A-Za-z0-9_]+\.rs/) {
        t = substr($0, RSTART + 14, RLENGTH - 17); next
      }
      /SDLC-GATE [A-Za-z0-9_.-]+ ran=[0-9]+/ {
        name = ""; ran = ""
        for (i = 1; i < NF; i++) if ($i == "SDLC-GATE") { name = $(i + 1); break }
        for (i = 1; i <= NF; i++) if ($i ~ /^ran=[0-9]+$/) { ran = substr($i, 5); break }
        if (name != "" && ran != "")
          printf "%s %s %s\n", (t == "" ? "<unattributed>" : t), name, ran
      }' "$log" | sort -u
  )
  zero_fatal=()
  zero_ok=()
  # Targets that ACTUALLY reported an environmental skip. This is the evidence
  # tolerance is granted on below -- not the mere fact of being listed.
  env_skipped=()
  for m in ${markers[@]+"${markers[@]}"}; do
    read -r mtarget mname mran <<<"$m"
    [ "$mran" = 0 ] || continue
    if in_list "$mtarget" ${tolerated[@]+"${tolerated[@]}"}; then
      zero_ok+=("$mname (tests/$mtarget.rs)")
      env_skipped+=("$mtarget")
    else
      zero_fatal+=("$mname (tests/$mtarget.rs)")
    fi
  done
  if [ ${#zero_fatal[@]} -gt 0 ]; then
    echo
    echo "FAIL[$tier]: ${#zero_fatal[@]} gate(s) reported ran=0 — they did not EXECUTE."
    for z in "${zero_fatal[@]}"; do echo "  - $z"; done
    echo "  A failure to EXECUTE, not a failure to pass: these asserted NOTHING, so the"
    echo "  tier's green says nothing about what they defend. Supply the missing input,"
    echo "  or — if the skip is genuinely environmental — name the target in"
    echo "  ENV_TOLERATED_$tier with the reason, where the skip stays visible and counted."
    rc=1
  fi
  if [ ${#zero_ok[@]} -gt 0 ]; then
    echo "gates that did NOT run : ${#zero_ok[@]}  (env-tolerated ran=0 — a skip, never a pass)"
    for z in "${zero_ok[@]}"; do echo "  - $z"; done
  fi

  # --- FAILURE TRIAGE against the quarantine ratchet -----------------------
  # EVERY harness kind, not just `--test`. This sed used to match one shape:
  #   error: test failed, to rerun pass `--test <name>`
  # cargo prints `--lib` for src/lib.rs unit tests, `--doc` for doctests,
  # `--bin <n>` for a binary's tests and `--bench <n>` for a bench target — none of
  # which produced an entry here. `unexpected` is built only from this list, so a
  # failing lib unit test, doctest or bin test was attributed to NOTHING and the
  # tier printed `ok[...]`. Measured on a synthetic tier log carrying a lib harness
  # with `3 failed`: `failed=3` printed, `0 failing` printed, exit 0.
  # Non---test kinds arrive as the `@`-prefixed pseudo-targets rerun_selector maps
  # back to a runnable command; they are quarantinable like any other target.
  mapfile -t failing < <(
    sed -n \
      -e 's/^error: test failed, to rerun pass `--test \([A-Za-z0-9_-]*\)`.*/\1/p' \
      -e 's/^error: test failed, to rerun pass `--bin \([A-Za-z0-9_-]*\)`.*/@bin:\1/p' \
      -e 's/^error: test failed, to rerun pass `--bench \([A-Za-z0-9_-]*\)`.*/@bench:\1/p' \
      -e 's/^error: test failed, to rerun pass `--lib`.*/@lib/p' \
      -e 's/^error: test failed, to rerun pass `--doc`.*/@doc/p' \
      "$log" | sort -u
  )

  # A CRASH is never tolerable, whatever the target's environmental status.
  #
  # Environmental tolerance exists for a real reason: g2_differential_mlir dlopens a
  # gitignored, possibly-stale in-tree .so, so it can fail LOCALLY for reasons that say
  # nothing about the code. But that reason covers a MISSING or STALE oracle — it does
  # not cover the compiler DYING on a fixture. Those are different events and only one
  # of them is environmental.
  #
  # Measured 2026-08-29: with g2 failing, a replay still printed "ALL TIERS OK /
  # GATE EXIT=0", because the tolerated check below `continue`s before `unexpected` is
  # built. The harness had just been fixed to DETECT a crash (MIND_CRASH); this line is
  # what stopped that detection from ever GATING anything.
  #
  # The harness prints an unmistakable marker when it classifies a crash, so tolerance
  # is withdrawn for exactly that case and left intact for the environmental one.
  crashed=()
  # The per-fixture marker is `MIND_CRASH <path>.mind  [reason]`. Anchor on that exact
  # shape: the SUMMARY line also carries the token (`... / 0 MIND_CRASH / ...`), and the
  # looser `MIND_CRASH [^0]` matched the `/` immediately after it -- so EVERY run that
  # printed a summary was reported as a crash, a pure divergence with `0 MIND_CRASH`
  # included. Right exit code, wrong cause, and it would send someone hunting a segfault
  # that never happened.
  if grep -qE "GATE FAILED: the pure-MIND compiler CRASHED|MIND_CRASH [A-Za-z0-9_/.-]+[.]mind" "$log" 2>/dev/null; then
    mapfile -t crashed < <(grep -oE "MIND_CRASH [A-Za-z0-9_/.-]+[.]mind" "$log" | awk '{print $2}' | sort -u)
    echo
    echo "FAIL[$tier]: the pure-MIND compiler CRASHED. Tolerance does NOT apply —"
    echo "  an unsupported construct returns a null handle; these terminated abnormally."
    for c in ${crashed[@]+"${crashed[@]}"}; do echo "  - $c"; done
    rc=1
  fi

  unexpected=()
  for t in "${failing[@]}"; do
    in_list "$t" ${quarantine[@]+"${quarantine[@]}"} && continue
    # Tolerance is granted ONLY for the outcome it exists for: the target could not
    # RUN here. The evidence is the target's own `ran=0` marker, collected into
    # `env_skipped` above.
    #
    # This was previously written the other way round: tolerance applied by DEFAULT
    # and was withdrawn for one named bad outcome (a crash). That is a denylist, and
    # it fails open on every outcome nobody thought to name. The one that mattered is
    # a byte-DIVERGENCE between the pure-MIND and Rust compilers: it prints no crash
    # marker, so `g2_differential_mlir` could report 99 diverging fixtures while this
    # tier printed `ok[exec]` and exited 0 -- the differential gate, whose entire
    # purpose is catching that disagreement, could not fail the build it gates.
    #
    # A target that RAN and produced findings is a real failure, whatever list it is
    # on. Environmental tolerance covers a missing oracle, never a wrong answer.
    if in_list "$t" ${tolerated[@]+"${tolerated[@]}"}; then
      if in_list "$t" ${env_skipped[@]+"${env_skipped[@]}"}; then
        continue
      fi
      unexpected+=("$t")
      continue
    fi
    unexpected+=("$t")
  done
  if [ ${#unexpected[@]} -gt 0 ]; then
    echo
    echo "FAIL[$tier]: ${#unexpected[@]} target(s) failed that are NOT quarantined:"
    for t in "${unexpected[@]}"; do
      echo "  - $t   (rerun: cargo test --no-default-features --features \"$features\" $(rerun_selector "$t"))"
    done
    rc=1
  fi

  # A quarantined target that now PASSES must leave the list, or the quarantine
  # quietly grows into a permanent excuse.
  fixed=()
  for t in ${quarantine[@]+"${quarantine[@]}"}; do
    in_list "$t" ${failing[@]+"${failing[@]}"} || fixed+=("$t")
  done
  if [ ${#fixed[@]} -gt 0 ]; then
    echo
    echo "FAIL[$tier]: ${#fixed[@]} quarantined target(s) now PASS — delete them from"
    echo "      QUARANTINE_$tier in scripts/exec_semantics_gate.sh (the list may only shrink):"
    for t in "${fixed[@]}"; do echo "  - $t"; done
    rc=1
  fi

  # --- AGGREGATE `failed` COUNT: asserted, not decorated -------------------
  # `failed` is summed from every `test result:` line and printed on the summary
  # line above — and until now it was compared to nothing, so a tier could print
  # `failed=3` and `ok[...]` in the same breath. It is asserted here rather than
  # left to the triage above because the two rest on DIFFERENT evidence: the
  # triage greps one cargo message whose wording cargo owns, this counts the
  # harness's own report. A cargo that reworded (or dropped) the rerun hint would
  # take the triage with it; the count survives.
  #
  # Attribution is per-harness so the shrink-only quarantine keeps working: a
  # failure inside a QUARANTINE_<tier> target, or inside a target that reported an
  # env-tolerated ran=0, is ACCOUNTED FOR. Everything else is not, and reds the
  # tier — which is the whole point.
  mapfile -t failed_blocks < <(
    awk '
      # Attribute at every compilation-unit boundary, mirroring the marker
      # consumer above so one target can never inherit another one'"'"'s identity.
      /^[[:space:]]*(Running|Doc-tests) / {
        key = "<unattributed>"
        if (match($0, /Running tests\/[A-Za-z0-9_-]+\.rs/))
          key = substr($0, RSTART + 14, RLENGTH - 17)
        else if ($0 ~ /Running unittests src\/lib\.rs/)
          key = "@lib"
        else if (match($0, /Running unittests src\/bin\/[A-Za-z0-9_-]+\.rs/))
          key = "@bin:" substr($0, RSTART + 26, RLENGTH - 29)
        else if ($0 ~ /Running unittests src\/main\.rs/) {
          # The bin NAME is not on this line; recover it from the deps binary
          # (`.../deps/mindc-9f8a…`) so this key matches the `@bin:<name>` the
          # rerun hint yields for the same failure.
          key = "@bin"
          if (match($0, /deps\/[A-Za-z0-9_-]+-[0-9a-f]+\)/)) {
            b = substr($0, RSTART + 5, RLENGTH - 6)
            sub(/-[0-9a-f]+$/, "", b)
            if (b != "") key = "@bin:" b
          }
        }
        else if (match($0, /Running benches\/[A-Za-z0-9_-]+\.rs/))
          key = "@bench:" substr($0, RSTART + 16, RLENGTH - 19)
        else if ($0 ~ /^[[:space:]]*Doc-tests /)
          key = "@doc"
        next
      }
      /^test result:/ {
        f = 0
        for (i = 1; i <= NF; i++) if ($i == "failed;") f = $(i - 1) + 0
        if (f > 0) print (key == "" ? "<unattributed>" : key) " " f
        key = ""
      }' "$log"
  )
  unaccounted=()
  unaccounted_n=0
  for fb in ${failed_blocks[@]+"${failed_blocks[@]}"}; do
    fkey="${fb%% *}"; fn="${fb##* }"
    in_list "$fkey" ${quarantine[@]+"${quarantine[@]}"} && continue
    in_list "$fkey" ${env_skipped[@]+"${env_skipped[@]}"} && continue
    unaccounted+=("$fkey reported $fn failing test(s)")
    unaccounted_n=$((unaccounted_n + fn))
  done
  if [ "$unaccounted_n" -gt 0 ]; then
    echo
    echo "FAIL[$tier]: $unaccounted_n test(s) FAILED in target(s) nothing accounts for."
    for u in "${unaccounted[@]}"; do echo "  - $u"; done
    echo "  The tier's own summary line prints this count; it is now asserted. A tier"
    echo "  cannot be green while a test it ran is red."
    rc=1
  fi

  # --- CARGO'S OWN EXIT STATUS: asserted, not printed ----------------------
  # It was captured and used exactly once, in an echo inside the failure branch —
  # so cargo could report failure while this gate reported success. A non-zero
  # exit is only acceptable when the log NAMES the target(s) responsible and the
  # ratchet above has already judged them; a non-zero exit with nothing to
  # attribute it to (a link error, a harness that aborted before printing its
  # result, a future cargo whose wording moved) fails closed.
  # `unknown` = a pre-marker log replayed through --from-log: unavailable, and
  # said so above, rather than assumed green.
  if [ "$cargo_status" != 0 ] && [ "$cargo_status" != unknown ] && [ ${#failing[@]} -eq 0 ] \
     && [ "$unaccounted_n" -eq 0 ]; then
    echo
    echo "FAIL[$tier]: cargo exited $cargo_status but no failing target could be named."
    echo "  Nothing in the log attributes it: no 'error: test failed, to rerun pass ...'"
    echo "  hint and no harness reporting failed>0. Something failed OUTSIDE the test"
    echo "  results (a link/build failure, an aborted harness, or a cargo message this"
    echo "  gate does not parse). Fail-closed: read the log, then teach the triage."
    echo "  full log: $log"
    rc=1
  fi

  # `crit_bad` is checked here too: it sets `overall` on its own (the two verdicts
  # are independent), but printing "ok[$tier]" alongside "GATE FAILED" would be the
  # same false-green shape this whole script exists to remove.
  if [ "$rc" = 0 ] && [ ${#crit_bad[@]} -eq 0 ]; then
    echo "ok[$tier]: $executed tests executed across $harnesses harnesses; ${#failing[@]} failing"
    echo "          target(s), all accounted for (quarantine ${#quarantine[@]}, env-tolerated ${#tolerated[@]})."
    # An env-tolerated ran=0 does not red the tier, but it must never vanish into
    # the ok line: the tier is green ABOUT LESS than it looks.
    if [ ${#zero_ok[@]} -gt 0 ]; then
      echo "          NOTE: ${#zero_ok[@]} gate(s) above reported ran=0 and asserted NOTHING."
    fi
  else
    echo
    echo "cargo exit was $cargo_status; failing targets: ${failing[*]:-none}"
    echo "full log: $log"
    for t in ${unexpected[@]+"${unexpected[@]}"}; do
      echo "--- tier '$tier' :: $t   (rerun: $(rerun_selector "$t")) ---"
      case "$t" in
        # A pseudo-target has no `Running tests/<t>.rs` block to anchor on: its
        # harness header is `Running unittests src/...` or `Doc-tests <crate>`.
        # Anchor on the harness's own failure list instead, or the reader gets an
        # empty excerpt for exactly the failures this gate was just taught to see.
        @*)
          awk '/^failures:$/ {on=1} on {print} on && /^test result:/ {exit}' "$log" | head -40 ;;
        *)
          awk -v t="tests/$t.rs" '$0 ~ ("Running " t) {on=1} on {print} on && /^test result:/ {exit}' "$log" \
            | grep -Ev '^test .* \.\.\. ok$' | head -40 ;;
      esac
    done
    overall=1
  fi
done

echo
if [ "$print_only" = 1 ]; then
  echo "--print-count: re-derive the floors from the numbers above; this mode never fails"
  exit 0
fi
if [ -n "$from_log" ]; then
  # No pass banner here, ever. --from-log ran no tests, so nothing it prints may
  # be greppable as proof that the tier is green.
  echo "ANALYSIS-ONLY (--from-log): judged a saved log; NO tests were run. rc=$overall"
  exit $overall
fi
[ "$overall" = 0 ] && echo "ALL TIERS OK" || echo "GATE FAILED — see FAIL[...] above"
exit $overall
