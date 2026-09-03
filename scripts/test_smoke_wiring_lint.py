#!/usr/bin/env python3
"""Regression test for the CI-coverage and branch-awareness contracts of
examples/mindc_mind/smoke_wiring_lint.py.

Two holes this pins shut, both of which let a GREEN lint certify an UNEXECUTED
gate:

  1. CI COVERAGE. The manifest legitimises `preflight` and `keystone` as
     runners, but no workflow invokes scripts/preflight.sh or
     examples/mindc_mind/fast_keystone.sh — so a `class=gate` row wired only to
     those two runs in nobody's CI. The lint checked that the manifest matched
     reality; it never asked whether reality contained CI. A gate may still opt
     out, but only by carrying a `deferred:` marker in its note that names the
     upgrade path.

  2. BRANCH AWARENESS. scripts/preflight.sh carried a comment asserting four
     gates "run UNCONDITIONALLY", while the gates sat inside the `--full`
     branch. The lint parsed preflight.sh as flat text, so it happily confirmed
     the runner column the false claim implied. The lint now knows where the
     `--full` branch starts and rejects an unconditional-execution claim made
     from inside it.

Hermetic: every case builds a miniature fixture tree (workflows + preflight +
smoke files + manifest) and runs the REAL lint against it, so this exercises the
shipped exit codes rather than a re-implementation.

Run: ``python3 scripts/test_smoke_wiring_lint.py`` (no third-party deps).
"""
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
LINT = REPO / "examples" / "mindc_mind" / "smoke_wiring_lint.py"
# The lint imports the verdict vocabulary from scripts/gate_assert.py rather than
# re-spelling it, and refuses to run at all without the verdict-shape residual
# list. A fixture repo holding only the lint is therefore not a repo the lint can
# run in -- it died on `ModuleNotFoundError: gate_assert` and every case reported
# the exit code of a crash instead of the exit code of a verdict. Both are copied
# in ALONGSIDE the lint, from the real tree, so the fixture keeps exercising the
# SHIPPED module set and cannot drift into testing a reduced one.
SHIM = REPO / "scripts" / "gate_assert.py"
# An EMPTY residual is legal; a MISSING one is not. The real tree's list names
# real gates, and copying it in would make every fixture case fail on "residual
# entry names nothing" -- an assertion about the fixture, never about the lint.
# `smoke_wiring_lint` is the ONE name carried over from the real list, because the
# fixture copies that exact file in as one of its gates and the real tree already
# declares it residual (it reports four whole-run verdicts, not one per case). The
# fixture's own smokes are empty files and are on no residual.
RESIDUAL_FIXTURE = (
    "# fixture corpus: only the copied-in lint is on the verdict-shape residual\n"
    "smoke_wiring_lint\n"
)

# A workflow that executes the two smokes named here, so the lint's vacuity
# guard ("no smoke is executed by any workflow") is satisfied in every case and
# each case tests only the property it names.
CI_YML = """\
jobs:
  gates:
    steps:
      - name: run the corpus
        run: |
          python3 examples/mindc_mind/smoke_wiring_lint.py
          python3 examples/mindc_mind/alpha_smoke.py
          python3 examples/mindc_mind/beta_smoke.py
"""

# beta runs in BOTH runners; gamma only in preflight (and only under --full);
# delta is invoked by nobody. That spread is what lets each case below isolate
# one property.
PREFLIGHT_CLAIM_OUTSIDE = """\
#!/usr/bin/env bash
# --- git-level SDLC gates ---
# These run UNCONDITIONALLY and BEFORE the mic@3 smoke.
python3 examples/mindc_mind/beta_smoke.py

if [ "${1:-}" = "--full" ]; then
  python3 examples/mindc_mind/gamma_smoke.py
fi
"""

PREFLIGHT_CLAIM_INSIDE = """\
#!/usr/bin/env bash
python3 examples/mindc_mind/beta_smoke.py

if [ "${1:-}" = "--full" ]; then
  # --- git-level SDLC gates ---
  # These run UNCONDITIONALLY and BEFORE the mic@3 smoke.
  python3 examples/mindc_mind/gamma_smoke.py
fi
"""

SMOKES = ("alpha_smoke", "beta_smoke", "gamma_smoke", "delta_smoke")


def build(tmp: Path, rows: list[tuple[str, str, str, str]], preflight: str) -> Path:
    """Materialise a fixture repo and return the path of the lint inside it."""
    smoke_dir = tmp / "examples" / "mindc_mind"
    smoke_dir.mkdir(parents=True)
    (tmp / ".github" / "workflows").mkdir(parents=True)
    (tmp / "scripts").mkdir(parents=True)
    (tmp / ".github" / "workflows" / "ci.yml").write_text(CI_YML, encoding="utf-8")
    (tmp / "scripts" / "preflight.sh").write_text(preflight, encoding="utf-8")
    for name in SMOKES:
        (smoke_dir / f"{name}.py").write_text("", encoding="utf-8")
    (smoke_dir / "SMOKE_WIRING.tsv").write_text(
        "# fixture manifest\n" + "".join("\t".join(r) + "\n" for r in rows),
        encoding="utf-8",
    )
    shutil.copyfile(LINT, smoke_dir / "smoke_wiring_lint.py")
    shutil.copyfile(SHIM, tmp / "scripts" / "gate_assert.py")
    (smoke_dir / "VERDICT_SHAPE_RESIDUAL.txt").write_text(
        RESIDUAL_FIXTURE, encoding="utf-8")
    return smoke_dir / "smoke_wiring_lint.py"


def run(rows: list[tuple[str, str, str, str]], preflight: str) -> tuple[int, str]:
    with tempfile.TemporaryDirectory() as td:
        lint = build(Path(td), rows, preflight)
        p = subprocess.run(
            [sys.executable, str(lint)], capture_output=True, text=True
        )
        return p.returncode, p.stdout + p.stderr


# Rows whose wiring already matches the fixture and whose classes are all
# legitimate; each case overrides exactly one of gamma/delta.
FIXED = [
    ("smoke_wiring_lint", "ci", "gate", ""),
    ("alpha_smoke", "ci", "gate", ""),
    ("beta_smoke", "ci,preflight", "gate", ""),
]
GAMMA_OK = ("gamma_smoke", "preflight", "tool", "Analysis only; prints numbers, no verdict.")
DELTA_OK = ("delta_smoke", "none", "helper", "Imported by the smokes; not a gate.")


def main() -> int:
    results: list[tuple[str, int, int, str]] = []

    def case(label, want, gamma=GAMMA_OK, delta=DELTA_OK, preflight=PREFLIGHT_CLAIM_OUTSIDE):
        rc, out = run(FIXED + [gamma, delta], preflight)
        ok = rc == want
        # ONE VERDICT LINE PER CASE, printed here rather than recapped at the end.
        # scripts/gate_assert.py derives `asserted=<N>` from reported verdicts, so
        # a single summary line publishes the same count whether this list holds
        # five cases or none -- emptying the case list would leave the count, and
        # the exit code, exactly as they are now.
        print(f"[{'PASS' if ok else 'FAIL'}] {label} (expected exit {want}, "
              f"got {rc})")
        results.append((label, want, rc, "" if ok else out))

    # Control: nothing wrong -> the lint still passes (the new rules did not
    # simply red everything).
    case("control: legitimate corpus -> exit 0", 0)

    # THE MUTATION the audit prescribes: a class=gate row wired to preflight
    # only. Executed by no workflow, so a regression in it lands CI-green.
    case("gate wired to preflight only -> exit 1 (runs in no CI)", 1,
         gamma=("gamma_smoke", "preflight", "gate",
                "Runs in preflight; nobody said why it is not in CI."))

    # Same hole one step further: a gate wired nowhere at all, with a note that
    # explains the gate but claims no deferral.
    case("gate wired nowhere, note without a deferral -> exit 1", 1,
         delta=("delta_smoke", "none", "gate",
                "Important cross-implementation gate; currently unwired."))

    # The exemption is real but must be EXPLICIT: a `deferred:` marker naming
    # the upgrade path keeps a red/unrunnable gate honestly recorded instead of
    # forcing it into CI or out of the tree.
    case("gate off CI WITH a deferred: marker -> exit 0 (justified)", 0,
         delta=("delta_smoke", "none", "gate",
                "RED at this commit. deferred: wire into CI once the divergence is fixed."))

    # Branch awareness: an "runs UNCONDITIONALLY" claim written INSIDE the
    # `--full` branch is false and must red the lint.
    case("preflight claims UNCONDITIONALLY from inside --full -> exit 1", 1,
         preflight=PREFLIGHT_CLAIM_INSIDE)

    failures = [r for r in results if r[3]]
    for label, want, rc, out in failures:
        for line in out.strip().splitlines()[:12]:
            print(f"    {line}")
    # The SANCTIONED count-marker shape. scripts/gate_assert.py reads a count only
    # as `SDLC-GATE <name> ran=<n> fail=<k>`; a bare `ran=/fail=` is a number the
    # gate typed rather than one it earned, and examples/mindc_mind/
    # smoke_wiring_lint.py refuses it for exactly that reason.
    print(f"\nSDLC-GATE smoke_wiring_lint_selftest ran={len(results)} "
          f"fail={len(failures)}")
    if failures:
        return 1
    print("test_smoke_wiring_lint: PASS — CI-coverage rule and preflight "
          "branch awareness both enforced")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
