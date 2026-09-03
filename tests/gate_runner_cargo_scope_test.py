#!/usr/bin/env python3
"""Gate test: the wiring lint must SEE a `cargo test` gate, or say it cannot.

Background — the defect this test exists to prevent
---------------------------------------------------
`scripts/run_gate.py` refuses a gate that exits 0 while publishing no
`asserted=<N>` evidence, and `scripts/gate_runner_wiring_lint.py` is what keeps
every workflow call site routed through it. But the lint only ever looked for a
repository SCRIPT — `(scripts|tests|tools|examples)/*.{py,sh}`. A workflow step
whose gate IS a `cargo test` invocation was invisible to it: not routed, not
required to be declared, and — the part that cost real integrity — not recorded
anywhere as UNCOVERED.

Two open defects live in exactly that blind spot and were at risk of being
recorded as closed by a wave that never touched them:

  * a capability skip inside a Rust test (`println!("... skipping"); return;`)
    makes a FAILING compile a PASSING test, and the honoured-everywhere
    `MIND_BENCH_REQUIRE` guard reaches 3 of 322 test files;
  * the differential fuzzer takes its program count from `MINDFUZZ_ITERS` with
    no floor, so `MINDFUZZ_ITERS=0` produces the digest of zero programs on both
    runners — equal, non-empty, and green on the cross-runner identity job.

Neither is fixable by the Python-only contract, and that is legitimate. What is
NOT legitimate is a meta-gate whose scan scope silently excludes a whole class
of gate while reporting "every gate invocation reaches the gate through the
runner". A scope a gate cannot see is a scope it must DECLARE.

The contract enforced here:
  1. a `cargo test` / `cargo bench` invocation in a workflow `run:` block is an
     invocation the lint reports, exactly like a script invocation;
  2. it passes only when routed through the runner, or when the workflow
     declares `# run_gate-exempt: cargo-test - <reason>`;
  3. for these, the reason must carry a `deferred:` marker naming the upgrade
     path — a cargo gate publishes no `asserted=N` line today, so its exemption
     is a temporary deferral, never a standing one;
  4. a declaration that outlives its call site fails, so the deferral cannot
     quietly survive the fix that closes it;
  5. the real workflow tree is clean AND non-vacuous: the lint reports a
     non-zero cargo-invocation count, because a scan that finds none is a broken
     pattern, not a clean tree.

Run:  python3 tests/gate_runner_cargo_scope_test.py
Exit: 0 = every case passed, 1 = a case failed (prints the offending case).
"""
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
LINT = REPO / "scripts" / "gate_runner_wiring_lint.py"

FAILURES: list[str] = []

DEFERRAL = (
    "deferred: cargo gates publish no asserted=N line; upgrade path is a "
    "shared skip helper plus a program-count floor"
)


def check(name: str, got: object, want: object) -> None:
    if got == want:
        print(f"[PASS] {name}")
    else:
        FAILURES.append(name)
        print(f"[FAIL] {name}\n       got  {got!r}\n       want {want!r}")


def run_lint(workflow_dir: Path) -> tuple[int, str]:
    proc = subprocess.run(
        [sys.executable, str(LINT), "--workflow-dir", str(workflow_dir)],
        capture_output=True, text=True,
    )
    return proc.returncode, proc.stdout + proc.stderr


def workflow(body: str) -> Path:
    """Materialise a one-step synthetic workflow and return its directory."""
    d = Path(tempfile.mkdtemp(prefix="cargo_scope_")) / "workflows"
    d.mkdir(parents=True)
    (d / "synthetic.yml").write_text(body, encoding="utf-8")
    return d


STEP = """\
name: synthetic
on: [push]
jobs:
  j:
    runs-on: ubuntu-latest
    steps:
      - name: a gate
        run: |
{body}
"""


def case_undeclared_cargo_test_is_reported() -> None:
    """The blind spot itself: an undeclared cargo gate must fail the lint."""
    d = workflow(STEP.format(body="          cargo test --test mindfuzz_cross_substrate"))
    rc, out = run_lint(d)
    check("undeclared cargo test fails", rc, 1)
    check("undeclared cargo test is named", "cargo-test" in out, True)


def case_declared_cargo_deferral_passes() -> None:
    """A deferral with a written upgrade path is the honest escape hatch."""
    body = (f"          # run_gate-exempt: cargo-test - {DEFERRAL}\n"
            "          cargo test --test mindfuzz_cross_substrate")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("declared cargo deferral passes", rc, 0)


def case_exemption_without_deferred_marker_fails() -> None:
    """A cargo exemption with no upgrade path is a standing hole, not a plan."""
    body = ("          # run_gate-exempt: cargo-test - this is fine and always will be, "
            "no plan attached\n"
            "          cargo test --test mindfuzz_cross_substrate")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("cargo exemption without `deferred:` fails", rc, 1)


def case_stale_cargo_declaration_fails() -> None:
    """When the fix lands and the cargo call goes, the deferral must go too."""
    body = (f"          # run_gate-exempt: cargo-test - {DEFERRAL}\n"
            "          echo no cargo here")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("stale cargo declaration fails", rc, 1)


def case_commented_cargo_line_is_not_an_invocation() -> None:
    """Prose about a gate is not a step that runs one.

    A routed script invocation rides along on purpose: with nothing at all to
    find, the lint fails closed on an empty scan, and this case would pass for
    the wrong reason.
    """
    body = ("          # $ cargo test --test something -- --nocapture\n"
            "          python3 scripts/run_gate.py scripts/check_claims.py")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("commented cargo line is not an invocation", rc, 0)
    check("commented cargo line is not counted", "cargo-test" in out, False)


def case_and_chained_direct_call_is_not_routed() -> None:
    """`routed` is a property of a SEGMENT, not of the whole joined command.

    `<routed call> && <direct call>` runs two gates: the first through the
    runner, the second past it. Asking whether the runner appears anywhere in
    the joined command answers yes for both, so the chained call is reported as
    routed -- the exact second way to run a gate this lint exists to forbid.
    """
    body = ("          python3 scripts/run_gate.py scripts/check_claims.py "
            "&& python3 scripts/check_release_gating.py")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("`&&`-chained direct call fails", rc, 1)
    check("`&&`-chained direct call is named",
          "scripts/check_release_gating.py: invoked DIRECTLY" in out, True)
    check("the routed half of the chain still passes",
          "[PASS] routed" in out and "scripts/check_claims.py" in out, True)


def case_semicolon_chained_direct_call_is_not_routed() -> None:
    """Same defect via `;` -- sequencing, not conjunction, is not the point."""
    body = ("          python3 scripts/run_gate.py scripts/check_claims.py"
            "; python3 examples/mindc_mind/mic3_flip_smoke.py")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("`;`-chained direct call fails", rc, 1)
    check("`;`-chained direct call is named",
          "examples/mindc_mind/mic3_flip_smoke.py: invoked DIRECTLY" in out, True)


def case_chained_direct_calls_are_counted_individually() -> None:
    """Both chained shapes in one workflow: two direct invocations, not zero."""
    body = ("          python3 scripts/run_gate.py scripts/check_claims.py "
            "&& python3 scripts/check_release_gating.py\n"
            "          python3 scripts/run_gate.py scripts/check_claims.py"
            "; python3 examples/mindc_mind/mic3_flip_smoke.py")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("chained direct calls fail the lint", rc, 1)
    check("both chained direct calls are reported", "2 problem(s)" in out, True)


def case_backgrounded_direct_call_before_runner_is_not_routed() -> None:
    """Segmenting alone is not enough — argument position carries the rest.

    `&` is deliberately NOT a segment separator (splitting it would cut `2>&1`
    in half), so a backgrounded direct call and a routed call land in ONE
    segment. Routing therefore also requires the path to appear AFTER the runner
    on that segment, i.e. as an argument to it; without that, an operator this
    lint does not split on would launder a direct call all over again.
    """
    body = ("          python3 scripts/check_release_gating.py "
            "& python3 scripts/run_gate.py scripts/check_claims.py")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("backgrounded direct call before the runner fails", rc, 1)
    check("backgrounded direct call is named",
          "scripts/check_release_gating.py: invoked DIRECTLY" in out, True)


def case_piped_routed_call_stays_routed() -> None:
    """Positive control against over-splitting.

    The real tree pipes a routed gate into `tee` inside an `if`. Segmenting must
    keep the gate and its path in ONE segment, or this lint would start failing
    call sites that are correctly routed -- a fix that trades a false pass for a
    false failure has not fixed anything.
    """
    body = ('          if python3 scripts/run_gate.py "examples/mindc_mind/$s.py" '
            "| tee /tmp/out.txt; then\n"
            "            echo ok\n"
            "          fi")
    rc, out = run_lint(workflow(STEP.format(body=body)))
    check("piped routed call stays routed", rc, 0)


def case_real_workflows_declare_their_cargo_gates() -> None:
    """The regression this wave must not re-open, on the real tree."""
    rc, out = run_lint(REPO / ".github" / "workflows")
    check("real workflow tree passes the extended lint", rc, 0)
    counted = 0
    for line in out.splitlines():
        if "cargo invocation" in line:
            for tok in line.replace("(", " ").replace(")", " ").split():
                if tok.isdigit():
                    counted = max(counted, int(tok))
    check("real tree reports a non-zero cargo-invocation count", counted > 0, True)


def main() -> int:
    for fn in sorted(
        (v for k, v in globals().items() if k.startswith("case_")),
        key=lambda f: f.__name__,
    ):
        fn()
    if FAILURES:
        print(f"\n{len(FAILURES)} FAILED: {', '.join(FAILURES)}")
        return 1
    print("\nRESULT: PASS — a cargo gate is inside the wiring lint's scope, "
          "and its deferral is written down")
    return 0


if __name__ == "__main__":
    sys.exit(main())
