#!/usr/bin/env python3
"""Regression test for scripts/exec_semantics_gate.sh: a FAILING test must red the tier.

THE HOLE THIS CLOSES
--------------------
The tier gate triaged failures with a sed anchored on exactly one shape:

    error: test failed, to rerun pass `--test <name>`

cargo prints `--lib`, `--doc`, `--bin <name>` and `--bench <name>` for the unit-test,
doctest, binary and benchmark harnesses, so a failing lib unit test, doctest or bin
test produced NO entry at all.  The verdict was built from that list alone, cargo's
own exit status was captured and only ever PRINTED, and the aggregate `failed` count
was printed and never asserted.  Measured on a synthetic tier log carrying a lib
harness with `3 failed`: the gate printed `failed=3` and then `ok[exec]: ... 0
failing` and exited 0.  A required release gate that cannot fail on a failing test.

WHAT THIS TEST DOES
-------------------
Each case synthesises a tier log (cheap: no cargo run, seconds not ~45 minutes),
feeds it to the real script through its ANALYSIS-ONLY `--from-log` mode, and asserts
the EXIT CODE.  Assertions are on the exit code, never on the prose, so a reworded
message cannot silently stop this test from gating.

The first case is a POSITIVE CONTROL: the same fixture with nothing wrong must exit
0.  Without it a red in every other case could just mean the fixture is malformed.
Two further cases assert the quarantine ratchet and the env-tolerated `ran=0` path
still behave exactly as before, so tightening the gate cannot be mistaken for
relaxing (or hardening) those.

Run: ``python3 scripts/test_exec_semantics_gate.py`` (no third-party deps).
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GATE = ROOT / "scripts" / "exec_semantics_gate.sh"

# Per-tier CRITICAL harnesses and floors, mirrored from the script.  Deliberately a
# small hand-written mirror: this test must keep working when the script's own
# parsing is what is under test, and a fixture that clears the floors by a wide
# margin does not need to track a floor change to the test.
CRITICAL = {
    "exec": ("alias_miscompile_run", "array_oob_trap_run"),
    "lowering": ("extern_c_phase_a", "extern_c_phase_b"),
    "pkg": ("package_basic", "package_traversal"),
}
# Targets QUARANTINE_<tier> names; a quarantined target that PASSES reds the tier,
# so a baseline fixture for that tier has to keep failing them.
QUARANTINED = {"exec": (), "lowering": ("std_surface_intrinsics",), "pkg": ()}

HARNESSES = 330  # every tier's harness floor is 310
PER_HARNESS = 7  # 330 * 7 = 2310 executed; the highest tier floor is 1900


def result_line(passed: int, failed: int = 0, ignored: int = 0) -> str:
    status = "FAILED" if failed else "ok"
    return (
        f"test result: {status}. {passed} passed; {failed} failed; "
        f"{ignored} ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    )


def block(header: str, passed: int, failed: int = 0, ignored: int = 0, body: str = "") -> str:
    total = passed + failed + ignored
    return (
        f"     {header}\n"
        f"\nrunning {total} tests\n"
        f"{body}"
        f"{result_line(passed, failed, ignored)}\n"
    )


def tier_log(
    tier: str,
    *,
    extra_blocks: str = "",
    error_lines: tuple[str, ...] = (),
    cargo_exit: int = 0,
    emit_exit_marker: bool = True,
) -> str:
    """A log that clears every floor of `tier`, plus whatever the case injects."""
    out = ["   Compiling libmind v0.1.0 (/w)\n    Finished test profile\n"]
    for name in CRITICAL[tier]:
        out.append(block(f"Running tests/{name}.rs (target/debug/deps/{name}-01)", PER_HARNESS))
    for name in QUARANTINED[tier]:
        # Quarantined targets must stay RED or the ratchet reds the tier itself.
        out.append(
            block(f"Running tests/{name}.rs (target/debug/deps/{name}-02)", PER_HARNESS - 1, 1)
        )
        error_lines = error_lines + (f"error: test failed, to rerun pass `--test {name}`",)
    filler = HARNESSES - len(CRITICAL[tier]) - len(QUARANTINED[tier])
    for i in range(filler):
        out.append(
            block(f"Running tests/filler_{i:03d}.rs (target/debug/deps/filler_{i:03d}-03)", PER_HARNESS)
        )
    out.append(extra_blocks)
    for line in error_lines:
        out.append(line + "\n")
    if emit_exit_marker:
        out.append(f"MIND_TIER_CARGO_EXIT={cargo_exit}\n")
    return "".join(out)


def run_gate(log_text: str, tier: str) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as td:
        log = Path(td) / f"mind-tier-{tier}.log"
        log.write_text(log_text, encoding="utf-8")
        return subprocess.run(
            ["bash", str(GATE), "--from-log", str(log), tier],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )


CASES: list[tuple[str, str, str, int]] = []


def case(name: str, tier: str, log_text: str, want_nonzero: bool) -> None:
    CASES.append((name, tier, log_text, 1 if want_nonzero else 0))


# --- 0. POSITIVE CONTROL -----------------------------------------------------
case("baseline exec log is GREEN (fixture sanity)", "exec", tier_log("exec"), False)

# --- 1..3. the three harness kinds cargo names with a non---test selector ----
case(
    "a failing lib UNIT test reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block(
            "Running unittests src/lib.rs (target/debug/deps/libmind-a1)",
            100,
            3,
            body="test ir::compact::v3::tests::roundtrip ... FAILED\n",
        ),
        error_lines=("error: test failed, to rerun pass `--lib`",),
        cargo_exit=101,
    ),
    True,
)
case(
    "a failing DOCTEST reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block("Doc-tests libmind", 40, 1),
        error_lines=("error: test failed, to rerun pass `--doc`",),
        cargo_exit=101,
    ),
    True,
)
case(
    "a failing BIN test reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block("Running unittests src/main.rs (target/debug/deps/mindc-b2)", 9, 2),
        error_lines=("error: test failed, to rerun pass `--bin mindc`",),
        cargo_exit=101,
    ),
    True,
)

# --- 4. cargo failed and NOTHING in the log explains it ----------------------
# Every harness reports ok and no rerun hint is printed, yet cargo exited 101 (a
# link failure, a harness that aborted before printing, a future cargo whose
# wording changed).  Fail closed: an unexplained non-zero exit is a failure.
case(
    "an UNATTRIBUTED non-zero cargo exit reds the tier",
    "exec",
    tier_log("exec", cargo_exit=101),
    True,
)

# --- 5. the aggregate failed count is asserted, not decorative ---------------
# No `error: test failed` line at all -- the shape the triage sed depends on is
# gone -- but a harness reported `2 failed`.  The count alone must red the tier,
# so the gate does not rest on one grep of one cargo message.
case(
    "a harness reporting failed>0 with NO rerun hint still reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block("Running tests/some_gate.rs (target/debug/deps/some_gate-c3)", 4, 2),
        cargo_exit=101,
    ),
    True,
)

# --- 6. the quarantine ratchet is preserved exactly --------------------------
# lowering's baseline log fails std_surface_intrinsics (a QUARANTINE_lowering
# entry) with a non-zero cargo exit.  That is the shape the gate is green on
# today and must stay green on: tightening must not turn the ratchet into a wall.
case(
    "a QUARANTINED failing target keeps the tier green",
    "lowering",
    tier_log("lowering", cargo_exit=101),
    False,
)

# --- 7. env-tolerated ran=0 is preserved exactly -----------------------------
case(
    "an ENV_TOLERATED ran=0 skip keeps the tier green",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block(
            "Running tests/g2_differential_mlir.rs (target/debug/deps/g2_differential_mlir-d4)",
            1,
            body="SDLC-GATE g2_differential_mlir ran=0 fail=0\n",
        ),
    ),
    False,
)

# --- 8. a NON-tolerated ran=0 still reds the tier ----------------------------
case(
    "a non-tolerated ran=0 marker still reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block(
            "Running tests/some_gate.rs (target/debug/deps/some_gate-e5)",
            1,
            body="SDLC-GATE some_gate ran=0 fail=0\n",
        ),
    ),
    True,
)


def main() -> int:
    if not GATE.is_file():
        print(f"FAIL: {GATE} not found", file=sys.stderr)
        return 2
    bad = 0
    for name, tier, log_text, want_nonzero in CASES:
        proc = run_gate(log_text, tier)
        got_nonzero = 1 if proc.returncode != 0 else 0
        ok = got_nonzero == want_nonzero
        # PASS, not `ok`: scripts/gate_assert.py counts a reported verdict
        # from a fixed token set, and `[ok]` is outside it -- every case here
        # published NO evidence, so the count marker below had nothing to
        # refine and the gate graded `asserted=0` once routed.
        verdict = "PASS" if ok else "FAIL"
        want = "non-zero" if want_nonzero else "0"
        print(f"[{verdict}] {name}: want exit {want}, got {proc.returncode}")
        if not ok:
            bad += 1
            sys.stdout.write(proc.stdout[-2500:])
            sys.stderr.write(proc.stderr[-2000:])
    # The sanctioned marker shape (see scripts/gate_assert.py MARKER_RES); a
    # bare `ran=/fail=` is refused by examples/mindc_mind/smoke_wiring_lint.py.
    print(f"\nSDLC-GATE exec_semantics_gate_selftest ran={len(CASES)} fail={bad}")
    if bad:
        print("FAIL: exec_semantics_gate.sh did not grade as required above")
        return 1
    print("OK: a failing lib/doc/bin test, an unattributed cargo exit and a bare")
    print("    failed>0 count each red the tier; quarantine and env-tolerance intact")
    return 0


if __name__ == "__main__":
    sys.exit(main())
