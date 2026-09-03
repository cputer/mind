#!/usr/bin/env python3
"""Gate test: the assertion count must be EVIDENCE, never a printed integer.

Background — the defect this test exists to prevent
---------------------------------------------------
`scripts/gate_assert.py` publishes the one number `scripts/run_gate.py` trusts:
`asserted=<N>`, refused when it is 0. That number was derivable from a line the
gate simply PRINTED — any `ran=<n>` or `scored=<n>` anywhere in the output
replaced every other piece of evidence. So a gate could report a healthy count
while checking nothing:

  * a smoke printing `(ran={len(CASES)})` reports the LENGTH OF A LIST, not the
    number of cases it executed — emptying its case loop left the count at 8;
  * a gate writing `asserted=99` to stderr outranked the shim's own line,
    because the runner concatenated stdout+stderr and took the last match;
  * a helper line that merely CONTAINED `ran=0` zeroed three real assertions.

The contract enforced here:
  1. a count marker is read only in the two shapes the repo already publishes
     (`SDLC-GATE <name> ran=<n> fail=<k>`, `scored=<n> divergences=<k>`);
  2. a marker never manufactures evidence — with zero evaluated asserts and
     zero verdict lines the count is 0, whatever the marker says;
  3. a gate that writes the contract line itself fails closed;
  4. gate sources may not print those shapes by hand (the lint in
     examples/mindc_mind/smoke_wiring_lint.py).

Run:  python3 tests/gate_assert_count_contract_test.py
Exit: 0 = every case passed, 1 = a case failed (prints the offending case).
"""
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SHIM = REPO / "scripts" / "gate_assert.py"
RUNNER = REPO / "scripts" / "run_gate.py"

sys.path.insert(0, str(REPO / "examples" / "mindc_mind"))
import smoke_wiring_lint  # noqa: E402

FAILURES: list[str] = []


def _run(src: str, *, args: list[str] | None = None) -> tuple[int, str, str]:
    """Run a synthetic gate through the shim.

    Returns (rc, stdout, stdout+stderr). The two streams are kept apart on
    purpose: the contract line lives on STDOUT and that is the only stream
    scripts/run_gate.py reads — folding stderr in is exactly the mistake that
    let a gate's own `asserted=99` on stderr outrank the shim's verdict.
    """
    with tempfile.TemporaryDirectory() as td:
        gate = Path(td) / "synthetic_gate.py"
        gate.write_text(src, encoding="utf-8")
        proc = subprocess.run(
            [sys.executable, str(SHIM), str(gate), *(args or [])],
            capture_output=True, text=True, cwd=str(REPO), timeout=120,
        )
    return proc.returncode, proc.stdout, proc.stdout + proc.stderr


def _asserted(out: str) -> int | None:
    """The count the shim published on stdout (its line is the last one)."""
    n: int | None = None
    for line in out.splitlines():
        s = line.strip()
        if s.startswith("asserted="):
            try:
                n = int(s.split("=", 1)[1])
            except ValueError:
                return None
    return n


def check(label: str, got: object, want: object) -> None:
    ok = got == want
    print(f"[{'PASS' if ok else 'FAIL'}] {label}: got={got!r} want={want!r}")
    if not ok:
        FAILURES.append(label)


# ── the shim's counting contract ───────────────────────────────────────────

def case_bare_marker_is_not_a_count() -> None:
    """A hand-printed `ran=5` with nothing behind it counts ZERO."""
    _, out, _ = _run('print("r" + "an=5")\n')
    check("bare marker, no evidence", _asserted(out), 0)


def case_bare_marker_never_overrides_evidence() -> None:
    """Two real verdict lines + a bare marker: the evidence wins, not the 5."""
    src = 'print("[PASS] leg one")\nprint("[PASS] leg two")\nprint("r" + "an=5")\n'
    _, out, _ = _run(src)
    check("bare marker beside 2 verdicts", _asserted(out), 2)


def case_helper_marker_cannot_zero_real_asserts() -> None:
    """A helper line containing `ran=0` must not discard evaluated asserts."""
    src = ('assert 1 == 1\nassert 2 == 2\nassert 3 == 3\n'
           'print("helper: r" + "an=0")\n')
    _, out, _ = _run(src)
    check("`ran=0` helper line beside 3 asserts", _asserted(out), 3)


def case_sdlc_marker_needs_evidence() -> None:
    """The sanctioned SDLC shape with no evidence behind it fails closed."""
    src = 'print("SDLC-G" + "ATE synthetic ran=7 fail=0")\n'
    _, out, allout = _run(src)
    check("SDLC marker alone", _asserted(out), 0)
    check("SDLC marker alone is diagnosed",
          "count marker without evidence" in allout, True)


def case_sdlc_marker_with_evidence_is_authoritative() -> None:
    src = ('print("[PASS] synthetic leg")\n'
           'print("SDLC-G" + "ATE synthetic ran=7 fail=0")\n')
    _, out, _ = _run(src)
    check("SDLC marker + evidence", _asserted(out), 7)


def case_tcdiff_marker_with_evidence_is_authoritative() -> None:
    src = ('print("RESULT: PASS — synthetic sweep")\n'
           'print("tcdiff synthetic sweep: sco" + "red=9 divergences=0")\n')
    _, out, _ = _run(src)
    check("tcdiff marker + evidence", _asserted(out), 9)


def case_tcdiff_zero_scored_is_zero() -> None:
    """`scored=0` is the sweep reporting it scored nothing — count 0."""
    src = ('print("RESULT: PASS — synthetic sweep")\n'
           'print("tcdiff synthetic sweep: sco" + "red=0 divergences=0")\n')
    _, out, _ = _run(src)
    check("tcdiff marker scored=0", _asserted(out), 0)


def case_forged_line_on_stderr_fails_closed() -> None:
    """A gate writing the contract line itself must not be believed."""
    src = ('import sys\nprint("[PASS] a real leg")\n'
           'print("asser" + "ted=99", file=sys.stderr)\n')
    _, out, allout = _run(src)
    check("forged count on stderr", _asserted(out), 0)
    check("forged count is diagnosed", "published its own" in allout, True)


def case_forged_line_on_stdout_fails_closed() -> None:
    src = ('print("[PASS] a real leg")\nprint("asser" + "ted=99")\n'
           'print("[PASS] another leg")\n')
    _, out, _ = _run(src)
    check("forged count on stdout", _asserted(out), 0)


def case_runner_ignores_a_forged_count() -> None:
    """End to end: run_gate.py must reject the forged gate, not report 99."""
    with tempfile.TemporaryDirectory() as td:
        gate = Path(td) / "forged_gate.py"
        gate.write_text('import sys\nprint("asser" + "ted=99", file=sys.stderr)\n',
                        encoding="utf-8")
        proc = subprocess.run(
            [sys.executable, str(RUNNER), str(gate)],
            capture_output=True, text=True, cwd=str(REPO), timeout=120,
        )
    check("run_gate rejects a forged count (rc)", proc.returncode, 1)
    check("run_gate did not read 99",
          "asserted=99 (<" not in proc.stdout + proc.stderr, True)


# ── the lint that keeps the shapes out of gate sources ─────────────────────

def _lint(src: str) -> list[str]:
    return smoke_wiring_lint.marker_violations(Path("synthetic_gate.py"), src)


def case_lint_flags_a_hand_printed_ran() -> None:
    check("lint: print(f\"(ran={n})\")",
          bool(_lint('n = 5\nprint(f"passed  (ran={n})")\n')), True)


def case_lint_flags_a_forged_asserted() -> None:
    check("lint: print(\"asserted=3\")",
          bool(_lint('print("asserted=3")\n')), True)


def case_lint_allows_the_two_families() -> None:
    sdlc = 'print(f"SDLC-GATE synthetic ran={t} fail={len(p)}")\n'
    tcd = ('print(f"tcdiff sweep: generated={g} scored={s} '
           'divergences={len(d)}")\n')
    check("lint: SDLC-GATE shape allowed", _lint(sdlc), [])
    check("lint: scored/divergences shape allowed", _lint(tcd), [])


def case_lint_allows_prose_mentioning_a_marker() -> None:
    """A docstring or comment naming a marker is not a gate printing one."""
    src = ('"""ran=<n>: the repo marker, described in prose."""\n'
           '# scored=5 divergences=0 in a comment\n'
           'print("[PASS] a real leg")\n')
    check("lint: prose mention allowed", _lint(src), [])


def case_lint_scans_the_real_corpus() -> None:
    """The corpus itself must be clean — this is the wave-1 regression."""
    bad = smoke_wiring_lint.scan_marker_violations()
    check("lint: corpus clean", bad, [])


def main() -> int:
    for fn in sorted(
        (v for k, v in globals().items() if k.startswith("case_")),
        key=lambda f: f.__name__,
    ):
        fn()
    if FAILURES:
        print(f"\n{len(FAILURES)} FAILED: {', '.join(FAILURES)}")
        return 1
    print("\nRESULT: PASS — the assertion count is evidence, not a printed integer")
    return 0


if __name__ == "__main__":
    sys.exit(main())
