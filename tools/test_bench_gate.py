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
    return run_capture(baseline, current).returncode


def run_capture(baseline: Path, current: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(GATE), "--baseline", str(baseline), "--current", str(current)],
        capture_output=True,
        text=True,
    )


def run_args(*argv: str) -> int:
    return subprocess.run(
        [sys.executable, str(GATE), *argv], capture_output=True, text=True
    ).returncode


def bencher(small_ns: int, medium_ns: int, large_ns: int, spread: int = 50) -> str:
    """A bencher-format current run with a spread small enough to be trusted."""
    return (
        f"test compiler_pipeline/parse_typecheck_ir/small_matmul ... bench:  {small_ns} ns/iter (+/- {spread})\n"
        f"test compiler_pipeline/parse_typecheck_ir/medium_mlp ... bench:  {medium_ns} ns/iter (+/- {spread})\n"
        f"test compiler_pipeline/parse_typecheck_ir/large_network ... bench:  {large_ns} ns/iter (+/- {spread})\n"
    )


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
        one_noisy = d / "one-noisy.out"
        one_noisy.write_text(
            "test compiler_pipeline/parse_typecheck_ir/small_matmul ... bench: 3000 ns/iter (+/- 50)\n"
            "test compiler_pipeline/parse_typecheck_ir/medium_mlp ... bench: 6100 ns/iter (+/- 80)\n"
            "test compiler_pipeline/parse_typecheck_ir/large_network ... bench: 14689 ns/iter (+/- 2157)\n"
        )

        cases = [
            ("empty current -> exit 4 (the vacuous-PASS hole)", run(base, empty), 4),
            ("partial current 2/3 -> exit 4 (no silent skip)", run(base, partial), 4),
            ("missing baseline -> exit 4 (no default substitution)", run(d / "nope.txt", good), 4),
            ("valid full run -> exit 0 (normal PASS preserved)", run(base, good), 0),
            ("one of three frontier fixtures noisy -> nonzero (no false green)", run(base, one_noisy), 2),
        ]
        for label, output, want in (
            ("full run reports asserted=3", run_capture(base, good).stdout, "asserted=3"),
            ("noisy fixture run reports asserted=2", run_capture(base, one_noisy).stdout, "asserted=2"),
        ):
            ok = want in output
            print(f"[{'PASS' if ok else 'FAIL'}] {label}")
            if not ok:
                failures.append(label)
        # --- THE ENFORCEMENT PATH ------------------------------------------
        # Every case above exercises a REFUSAL to measure (exit 4) or the happy
        # path. Not one drove the gate to its actual FAIL verdict, so the only
        # branch that can stop a regression from landing had no test at all.
        # A gate whose enforcement is untested is a gate nobody has watched fail.
        regressed = d / "regressed.out"
        regressed.write_text(bencher(3600, 7400, 20200))   # ~+20% on every bench
        within = d / "within.out"
        within.write_text(bencher(3120, 6380, 17480))      # ~+4%, under the 10% default
        faster = d / "faster.out"
        faster.write_text(bencher(2700, 5500, 15100))      # a win: one-sided, must pass
        # ALL three above the variance threshold: a uniformly loaded box.
        all_noisy = d / "all_noisy.out"
        all_noisy.write_text(
            "test compiler_pipeline/parse_typecheck_ir/small_matmul ... bench:  3600 ns/iter (+/- 900)\n"
            "test compiler_pipeline/parse_typecheck_ir/medium_mlp ... bench:  7400 ns/iter (+/- 1850)\n"
            "test compiler_pipeline/parse_typecheck_ir/large_network ... bench:  20200 ns/iter (+/- 5050)\n"
        )
        # MIXED: two benches too noisy to trust, one clean and regressed. The
        # first draft of this test assumed a single spread made all three noisy;
        # it did not (900/20200 is 4.5%), the gate gated on the one trustworthy
        # bench, and the test caught its own wrong expectation rather than the
        # gate's behaviour. Both shapes are asserted now, because "ignored the
        # noise, enforced on what it could trust" is the behaviour that matters
        # on a shared machine.
        mixed_noisy = d / "mixed_noisy.out"
        mixed_noisy.write_text(bencher(3600, 7400, 20200, spread=900))

        cases += [
            ("+20% regression -> exit 1 (THE enforcement path)",
             run_args("--baseline", str(base), "--current", str(regressed)), 1),
            ("+4% under threshold -> exit 0",
             run_args("--baseline", str(base), "--current", str(within)), 0),
            ("faster than baseline -> exit 0 (gate is one-sided)",
             run_args("--baseline", str(base), "--current", str(faster)), 0),
            ("+20% with EVERY bench over the spread limit -> exit 2 (loaded box is "
             "INCONCLUSIVE, not a regression)",
             run_args("--baseline", str(base), "--current", str(all_noisy)), 2),
            ("+20% with 2 noisy and 1 trustworthy -> exit 1 (enforce on what can be trusted)",
             run_args("--baseline", str(base), "--current", str(mixed_noisy)), 1),
        ]

        # --- THE CHAMPION RATCHET ------------------------------------------
        # The champion is the whole point of the tool -- it catches slow drift
        # that never trips the floor: ten commits at +3% each pass any
        # stale-floor check and lose 30% together. `--champion` appeared ZERO
        # times across .github/workflows, so the ratchet the tool advertises had
        # never run, and nothing here tested it either.
        #
        # This case is exactly that shape: current EQUALS the floor (so the floor
        # is satisfied and would pass) while sitting ~20% above the champion.
        champ = d / "champion.txt"
        champ.write_text(
            "compiler_pipeline/parse_typecheck_ir/small_matmul:   2.508 µs\n"
            "compiler_pipeline/parse_typecheck_ir/medium_mlp:     5.222 µs\n"
            "compiler_pipeline/parse_typecheck_ir/large_network: 12.231 µs\n"
        )
        champbeat = d / "champbeat.out"
        champbeat.write_text(bencher(2400, 5000, 11800))

        import importlib.util as _ilu2
        _s2 = _ilu2.spec_from_file_location("bench_gate_ref", GATE)
        _bg2 = _ilu2.module_from_spec(_s2)
        _s2.loader.exec_module(_bg2)
        champ_keys = len(_bg2.parse_reference(champ))

        ratchet = ["--champion", str(champ), "--floor", str(base),
                   "--floor-prefix", "compiler_pipeline/parse_typecheck_ir"]
        cases += [
            ("champion in full-id prose parses to 3 keys (it parsed to 0 before)",
             champ_keys, 3),
            ("at the floor but +20% over champion -> exit 1 (the ratchet, not the floor)",
             run_args(*ratchet, "--current", str(good)), 1),
            ("beats the champion -> exit 0 (the ratchet is one-sided too)",
             run_args(*ratchet, "--current", str(champbeat)), 0),
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
