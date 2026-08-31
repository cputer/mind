#!/usr/bin/env python3
"""Regression test for the bench-gate fail-closed contract (tools/bench_gate.py).

Guards the vacuous-PASS hole: an empty / truncated / partial ``bench.out``, or a
missing baseline file, must exit NONZERO (code 4 = malformed gate input) — never
fall through to ``bench gate: PASS``. A uniformly-noisy run still returns its
inconclusive code, not a false green.

Run: ``python3 tools/test_bench_gate.py`` (no third-party deps).
"""
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
GATE = HERE / "bench_gate.py"

GOOD_CURRENT = (
    "test compiler_pipeline/parse_typecheck_ir/small_matmul ... bench:  3000 ns/iter (+/- 50)\n"
    "test compiler_pipeline/parse_typecheck_ir/medium_mlp ... bench:  6100 ns/iter (+/- 80)\n"
    "test compiler_pipeline/parse_typecheck_ir/large_network ... bench:  16800 ns/iter (+/- 120)\n"
)
GOOD_BASELINE = "small_matmul:   3.00 µs\nmedium_mlp:   6.13 µs\nlarge_network:   16.82 µs\n"


def run(baseline: Path, current: Path) -> int:
    return subprocess.run(
        [sys.executable, str(GATE), "--baseline", str(baseline), "--current", str(current)],
        capture_output=True,
        text=True,
    ).returncode


def main() -> int:
    failures: list[str] = []
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        base = d / "baseline.txt"
        base.write_text(GOOD_BASELINE)
        good = d / "good.out"
        good.write_text(GOOD_CURRENT)
        empty = d / "empty.out"
        empty.write_text("")
        partial = d / "partial.out"
        partial.write_text("\n".join(GOOD_CURRENT.splitlines()[:2]) + "\n")

        cases = [
            ("empty current -> exit 4 (the vacuous-PASS hole)", run(base, empty), 4),
            ("partial current 2/3 -> exit 4 (no silent skip)", run(base, partial), 4),
            ("missing baseline -> exit 4 (no default substitution)", run(d / "nope.txt", good), 4),
            ("valid full run -> exit 0 (normal PASS preserved)", run(base, good), 0),
        ]
        # --- transition-line parsing (the superseded-number bug) ------------
        # A real baseline file narrates the re-baseline in prose BEFORE stating
        # the frozen numbers:
        #
        #   small_matmul:   2.80 µs -> 2.98 µs   (+6.4%)   <- OLD -> NEW prose
        #   ...
        #   small_matmul:   2.98 µs   (3-run: ...)          <- the frozen number
        #
        # REF_LINE used to match the prose line first and setdefault() locked in
        # its LEFT-hand (superseded) value, so the gate silently enforced the
        # numbers the file exists to REPLACE. Assert the frozen block wins.
        import importlib.util as _ilu
        _spec = _ilu.spec_from_file_location(
            "bench_gate", Path(__file__).resolve().parent / "bench_gate.py"
        )
        _bg = _ilu.module_from_spec(_spec)
        _spec.loader.exec_module(_bg)

        rebaselined = d / "rebaselined.txt"
        rebaselined.write_text(
            "Net effect vs the prior baseline:\n"
            "\n"
            "  small_matmul:   2.80 µs \u2192 2.98 µs   (+6.4%)\n"
            "  medium_mlp:     6.55 µs \u2192 6.93 µs   (+5.8%)\n"
            "  large_network: 17.10 µs \u2192 18.43 µs  (+7.8%)\n"
            "\n"
            "Frozen reference measurement:\n"
            "\n"
            "  small_matmul:   2.98 µs   (3-run: 2.93 / 2.98 / 3.16)\n"
            "  medium_mlp:     6.93 µs   (3-run: 6.80 / 6.93 / 7.28)\n"
            "  large_network:  18.43 µs  (3-run: 17.95 / 18.43 / 19.52)\n"
        )
        parsed = _bg.parse_reference(rebaselined)
        want_frozen = {"small_matmul": 2.98, "medium_mlp": 6.93, "large_network": 18.43}
        for name, want_v in want_frozen.items():
            got_v = parsed.get(name)
            ok = got_v == want_v
            label = f"frozen block wins over prose transition line ({name})"
            print(f"[{'PASS' if ok else 'FAIL'}] {label} (got {got_v}, want {want_v})")
            if not ok:
                failures.append(label)

        # And the same check against the REAL committed baseline the CI gate
        # uses, so a future edit to that file cannot reintroduce the bug.
        real = Path(__file__).resolve().parent.parent / ".bench-baseline-2026-06-01-correctness.txt"
        if real.exists():
            rp = _bg.parse_reference(real, prefix="compiler_pipeline/parse_typecheck_ir")
            for name, want_v in want_frozen.items():
                key = f"compiler_pipeline/parse_typecheck_ir/{name}"
                got_v = rp.get(key)
                ok = got_v == want_v
                label = f"committed baseline parses to frozen value ({name})"
                print(f"[{'PASS' if ok else 'FAIL'}] {label} (got {got_v}, want {want_v})")
                if not ok:
                    failures.append(label)

        for label, got, want in cases:
            ok = got == want
            print(f"[{'PASS' if ok else 'FAIL'}] {label} (got {got}, want {want})")
            if not ok:
                failures.append(label)

    if failures:
        print(f"\n{len(failures)} bench-gate contract test(s) FAILED")
        return 1
    print("\nbench-gate fail-closed contract: all cases OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
