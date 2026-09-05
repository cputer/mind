# shellcheck shell=bash
# exec_semantics_markers.sh — the two ran=0 verdicts of the executable-semantics
# tier gate, carried as a sourced library.
#
# Sourced by scripts/exec_semantics_gate.sh, never executed: function
# definitions only, no side effects at source time, no cd, no exit.
#
# WHY THIS IS A SEPARATE FILE
# ---------------------------
# The house rule is 200-400 lines typical, 800 max, and "do not grow an
# over-limit file — carry the new logic into a focused module". The gate script
# crossed the ceiling (729 -> 843) while these two verdicts were being taught to
# read the marker's `class=` field and to fail on dead tolerance, and nothing
# mechanical noticed: tests/module_size_ratchet.rs walked src/ only, so the
# ceiling was enforced everywhere EXCEPT where the gate work was happening.
# Both halves of that are fixed together — the ratchet now reads every tracked
# .rs/.py/.sh source root off `git ls-files`, and this is the seam it demanded.
#
# WHAT BELONGS HERE
# -----------------
# Exactly the two checks that answer "did the gate RUN", from the marker
# evidence:
#
#   consume_skip_markers  — every `SDLC-GATE <name> ran=<n> class=<c>` line in a
#       tier log, attributed to its compilation unit and triaged into fatal /
#       env-tolerated / class-legal.
#   check_dead_tolerance  — the shrink-ratchet on ENV_TOLERATED_<tier>: a
#       tolerated name the tier never BUILT is tolerance for nothing.
#
# They share `in_list`, and the second consumes nothing the first produces, so
# the seam is between them and the tier loop rather than through either one.
# The floors, the CRITICAL_<tier> minimums, the failure triage and the verdict
# stay in the gate: they read cargo's output, not the markers, and splitting a
# verdict away from the counts it is judged against is how a gate stops being
# readable as one rule.
#
# CONTRACT
# --------
# Both functions PRINT their own FAIL block and RETURN 1 on failure, 0 otherwise
# — the caller folds that into its own `rc`. `consume_skip_markers` additionally
# sets four arrays in the CALLER's scope, deliberately un-`local`:
#
#   zero_fatal     ran=0 that fails the tier
#   zero_ok        ran=0 excused by ENV_TOLERATED_<tier>
#   zero_expected  ran=0 whose class= the gate helper declares legal here
#   env_skipped    the TARGETS behind zero_ok — the evidence the failure triage
#                  grants environmental tolerance on, so a tolerated target that
#                  RAN and failed is still a real failure
#
# The names are the gate's own; the prose in both files refers to them by name,
# so they are the interface rather than an implementation detail to be renamed.

# True when $1 is one of the remaining arguments.
in_list() { local n=$1; shift; local e; for e in "$@"; do [ "$e" = "$n" ] && return 0; done; return 1; }

# consume_skip_markers <log> <tier> <require_toolchain> [env-tolerated-target...]
consume_skip_markers() {
  local log=$1 tier=$2 require_toolchain=$3
  shift 3
  local tolerated=("$@")
  local ret=0
  local -a markers
  local m mtarget mname mran mclass z
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
  # THIS CONSUMER READ NOTHING UNTIL THE MARKER SURVIVED CAPTURE. libtest
  # discards a PASSING test's stdout, and a capability skip PASSES, so every
  # marker written with `println!` was thrown away before this awk ever saw the
  # log: measured on tests/std_mlir_bindings_smoke.rs, `ok. 4 passed` and ZERO
  # `SDLC-GATE` lines, with the skip having happened. Every `gate::skipped_optional`
  # was an invisible PASS in every tier. tests/common/gate.rs now writes the
  # marker to the PROCESS stdout handle, which libtest does not shim.
  # `-- --show-output` was the other candidate and is REJECTED: it splices every
  # passing test's captured stdout into this same log, and some tests here print
  # a subprocess `mindc test` summary starting with `test result:` — measured,
  # it added 5 phantom harnesses and 9 phantom executed tests (one of them a
  # phantom FAILURE) to both `lowering` and `pkg`, corrupting the floors and the
  # aggregate `failed` assert below.
  #
  # WHICH ran=0 IS FATAL is decided from the marker's own `class=` field crossed
  # with this tier's REQUIRE_TOOLCHAIN knob, never from prose and never from a
  # hand-copied list of names. See the `zero_expected` note below.
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
        name = ""; ran = ""; cls = ""
        for (i = 1; i < NF; i++) if ($i == "SDLC-GATE") { name = $(i + 1); break }
        for (i = 1; i <= NF; i++) if ($i ~ /^ran=[0-9]+$/) { ran = substr($i, 5); break }
        # The absence CLASS the producer stamped (tests/common/gate.rs::Absent).
        # A producer that stamps none is `unclassified` and stays FAIL-CLOSED
        # below — a marker may never buy tolerance by omitting a field.
        for (i = 1; i <= NF; i++) if ($i ~ /^class=[a-z]+$/) { cls = substr($i, 7); break }
        if (name != "" && ran != "")
          printf "%s %s %s %s\n", (t == "" ? "<unattributed>" : t), name, ran,
                                   (cls == "" ? "unclassified" : cls)
      }' "$log" | sort -u
  )
  zero_fatal=()
  zero_ok=()
  # A ran=0 whose CLASS the gate helper already declared legal here. Counted and
  # printed on every run, never fatal -- and never a SECOND rule, which is the
  # whole reason the class travels in the marker:
  #
  #   class=optional   tests/common/gate.rs::is_fail_closed answers NO for
  #       `Absent::OptionalInput` in EVERY tier -- MIND_BENCH_REQUIRE says "use a
  #       real backend", not "install everything", and the exec tier's own
  #       tests/fail_closed_capability_skip_env.rs pins that an opt-in corpus or
  #       opt-in silicon survives enforcement. A tier that failed on it would be
  #       a second, contradictory owner of one rule. What was MISSING was never
  #       fatality, it was VISIBILITY: the marker never reached this log at all,
  #       so `skipped_optional` graded as a silent PASS.
  #   class=toolchain in a REQUIRE_TOOLCHAIN_<tier>=0 tier
  #       REQUIRE_TOOLCHAIN_<tier> IS this tier's answer to "may a toolchain
  #       absence skip here", and gate.rs applies the identical rule per-process
  #       through MIND_BENCH_REQUIRE. `lowering` and `pkg` answer 0 because they
  #       build WITHOUT mlir-build on purpose, so phase_g_keystone_bootstrap and
  #       mindc_cache_phase_f cannot run there BY CONSTRUCTION. `exec` answers 1,
  #       where the helper PANICS instead of marking -- so a toolchain marker
  #       arriving there anyway came from a producer that did not consult the
  #       rule, and stays FATAL below.
  #
  # Neither arm is named in ENV_TOLERATED, deliberately: that list ALSO excuses a
  # target's FAILURES (see the triage below), so putting the cross-substrate
  # canary gate on it to permit its deferred VNNI rung would have made a real
  # canary divergence unaccounted-for. Two different permissions, and only one of
  # them is being granted here.
  zero_expected=()
  # Targets that ACTUALLY reported an environmental skip. This is the evidence
  # tolerance is granted on below -- not the mere fact of being listed.
  env_skipped=()
  for m in ${markers[@]+"${markers[@]}"}; do
    read -r mtarget mname mran mclass <<<"$m"
    [ "$mran" = 0 ] || continue
    if in_list "$mtarget" ${tolerated[@]+"${tolerated[@]}"}; then
      zero_ok+=("$mname (tests/$mtarget.rs)")
      env_skipped+=("$mtarget")
    elif [ "$mclass" = optional ] \
      || { [ "$mclass" = toolchain ] && [ "$require_toolchain" != 1 ]; }; then
      zero_expected+=("$mname (tests/$mtarget.rs)")
    else
      zero_fatal+=("$mname (tests/$mtarget.rs)")
    fi
  done
  if [ ${#zero_fatal[@]} -gt 0 ]; then
    echo
    echo "FAIL[$tier]: ${#zero_fatal[@]} gate(s) reported ran=0 — they did not EXECUTE."
    for z in "${zero_fatal[@]}"; do echo "  - $z"; done
    echo "  A failure to EXECUTE, not a failure to pass: these asserted NOTHING, so the"
    echo "  tier's green says nothing about what they defend. Each is one of two shapes:"
    echo "    * class=toolchain here, where REQUIRE_TOOLCHAIN_$tier=$require_toolchain demands a real"
    echo "      backend — install the toolchain; a skip asserts NOTHING. (The gate helper"
    echo "      PANICS on this, so a marker means a producer that never read the rule.)"
    echo "    * no class= field at all — a producer outside tests/common/gate.rs. Stamp its"
    echo "      absence class, or — if the skip is genuinely environmental — name the"
    echo "      target in ENV_TOLERATED_$tier with the reason, where it stays counted."
    ret=1
  fi
  if [ ${#zero_ok[@]} -gt 0 ]; then
    echo "gates that did NOT run : ${#zero_ok[@]}  (env-tolerated ran=0 — a skip, never a pass)"
    for z in "${zero_ok[@]}"; do echo "  - $z"; done
  fi
  if [ ${#zero_expected[@]} -gt 0 ]; then
    echo "gates that did NOT run : ${#zero_expected[@]}  (class the helper declares legal here — a skip, never a pass)"
    for z in "${zero_expected[@]}"; do echo "  - $z"; done
  fi
  return $ret
}

# check_dead_tolerance <log> <tier> <harnesses> <floor_h> [env-tolerated-target...]
check_dead_tolerance() {
  local log=$1 tier=$2 harnesses=$3 floor_h=$4
  shift 4
  local tolerated=("$@")
  local ret=0
  local -a tier_targets dead_tolerance
  local tname d
  # --- TOLERANCE SHRINK-RATCHET (the missing half of the quarantine ratchet) --
  # A quarantined target that starts PASSING must leave its list, or the
  # quarantine "quietly grows into a permanent excuse". ENV_TOLERATED had no such
  # rule, so an entry naming a target this tier does not even BUILD produced no
  # signal at all — the mindfuzz entries sat dead until a human noticed them.
  # Dead tolerance is not harmless: it is pre-granted permission for a target to
  # report ran=0 the day it becomes buildable here, with nobody deciding that.
  #
  # The test is the tier's OWN log, not a hand-copied expectation: cargo prints
  # `Running tests/<t>.rs` for every target it built, so a tolerated name absent
  # from that set is tolerance for something this tier never ran.
  mapfile -t tier_targets < <(
    grep -oE 'Running tests/[A-Za-z0-9_]+\.rs' "$log" | sed -e 's|Running tests/||' -e 's|\.rs$||' | sort -u
  )
  dead_tolerance=()
  for tname in ${tolerated[@]+"${tolerated[@]}"}; do
    in_list "$tname" ${tier_targets[@]+"${tier_targets[@]}"} || dead_tolerance+=("$tname")
  done
  if [ ${#dead_tolerance[@]} -gt 0 ] && [ "$harnesses" -ge "$floor_h" ]; then
    echo
    echo "FAIL[$tier]: ENV_TOLERATED_$tier names ${#dead_tolerance[@]} target(s) this tier never built."
    for d in "${dead_tolerance[@]}"; do echo "  - $d"; done
    echo "  Tolerance for a target that does not run here is dead configuration: it"
    echo "  cannot excuse anything today and silently pre-approves a ran=0 the day the"
    echo "  target becomes buildable. Remove the entry (structural absence beats runtime"
    echo "  tolerance), or restore the target to this tier."
    ret=1
  fi
  return $ret
}
