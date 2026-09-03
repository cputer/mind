#!/usr/bin/env python3
"""Gate test: a count PRINTED on a public surface must equal the derived count.

Background — the defect this test exists to prevent
---------------------------------------------------
`[counts]` in config/capabilities.toml compared the MANIFEST's `declared`
number against the tree, and nothing ever read the number a reader actually
sees. README.md typed "~1,390 tests across 174 test files" while the tree held
325 files under tests/ and 2,217 `#[test]` functions; the manifest declared a
FLOOR of 156, so `156 <= 325` reported `[PASS]`, the gate exited 0, and the
surface number was free to drift by 151 files unchallenged. The manifest's own
note quoted README text ("... across 156 test files") that no longer existed —
a floor comparison stays green precisely while the surface and the tree drift
apart.

The fix is a second leg in `scripts/check_claims.py`: an entry may declare
`surface_regex`, and the number that regex captures ON THE SURFACE is compared
to the DERIVED count. This test is the proof that the comparison is
load-bearing: it mutates the surface number, the surface sentence and the
surface file in turn and requires the gate to go red for each, and it asserts
the LIVE manifest actually wires that leg to the README claims (a new check
kind nothing uses is not a gate).

Run:  python3 tests/check_claims_surface_count_gate_test.py
Exit: 0 = all cases pass, 1 = a case failed (prints the offending case).
"""
from __future__ import annotations

import os
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CHECK = REPO / "scripts" / "check_claims.py"
CAPS = REPO / "config" / "capabilities.toml"

# Fixture tree size. Small and fixed: the point is the comparison, not the tree.
FIXTURE_FILES = 12

MANIFEST_TEMPLATE = """
[ir]
canonical_text   = "mic@1"
canonical_binary = "mic@3"

[counts.fixture_files]
declared          = 3
mode              = "floor"
kind              = "glob_count"
globs             = ["tests/**/*.rs"]
surface           = "README.md"
surface_regex     = '([\\d,]+)\\+ test files'
surface_mode      = "{surface_mode}"
surface_tolerance = {surface_tolerance}
note              = "fixture"
"""

README_TEMPLATE = """# Fixture surface

Canonical IR: mic@1 text, mic@3 binary.

{sentence}
"""


def write_tree(root: Path, sentence: str, n_files: int = FIXTURE_FILES) -> None:
    (root / "tests").mkdir(parents=True, exist_ok=True)
    for i in range(n_files):
        (root / "tests" / f"t{i:03d}.rs").write_text(
            "#[test]\nfn t() {}\n", encoding="utf-8"
        )
    (root / "README.md").write_text(
        README_TEMPLATE.format(sentence=sentence), encoding="utf-8"
    )


def run_gate(root: Path, caps: Path, surfaces: str = "README.md") -> tuple[int, str]:
    env = dict(os.environ)
    env["CHECK_CLAIMS_ROOT"] = str(root)
    env["CHECK_CLAIMS_CAPS"] = str(caps)
    env["CHECK_CLAIMS_SURFACES"] = surfaces
    proc = subprocess.run(
        [sys.executable, str(CHECK)],
        env=env, capture_output=True, text=True, check=False,
    )
    return proc.returncode, proc.stdout + proc.stderr


def case(
    label: str,
    *,
    sentence: str,
    want_red: bool,
    surface_mode: str = "floor",
    surface_tolerance: int = 0,
    n_files: int = FIXTURE_FILES,
    drop_surface: bool = False,
) -> bool:
    """One fixture run. Returns True when the gate behaved as required."""
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        write_tree(root, sentence, n_files)
        surfaces = "README.md"
        if drop_surface:
            (root / "README.md").unlink()
            # The forbidden-phrase leg needs SOME surface, or the gate exits 2
            # ("no surfaces found") for an unrelated reason and proves nothing.
            (root / "OTHER.md").write_text(
                "Canonical IR: mic@1 text, mic@3 binary.\n", encoding="utf-8"
            )
            surfaces = "OTHER.md"
        caps = root / "caps.toml"
        caps.write_text(
            MANIFEST_TEMPLATE.format(
                surface_mode=surface_mode, surface_tolerance=surface_tolerance
            ),
            encoding="utf-8",
        )
        rc, out = run_gate(root, caps, surfaces)

    red = rc != 0
    right_reason = "DRIFT [counts/fixture_files]" in out
    ok = red == want_red and (right_reason if want_red else True)
    print(f"[{'PASS' if ok else 'FAIL'}] {label}: rc={rc} want_red={want_red}")
    if not ok:
        print(out)
    return ok


def live_wiring() -> bool:
    """The LIVE manifest must wire the surface leg to the README test counts.

    Without this, every case above could pass against a fixture while the real
    README claim stayed unparsed — the exact hole the fixture cases describe.
    """
    caps = tomllib.loads(CAPS.read_text(encoding="utf-8"))
    counts = caps.get("counts", {})
    ok = True
    for name in ("rust_test_files", "rust_test_fns"):
        spec = counts.get(name, {})
        wired = bool(spec.get("surface_regex"))
        print(f"[{'PASS' if wired else 'FAIL'}] live manifest: counts[{name}] "
              f"parses its surface number")
        ok &= wired
        if not wired:
            continue
        # The regex must actually match the declared surface TODAY: a pattern
        # that matches nothing is a check that asserts nothing.
        surface = REPO / spec["surface"]
        hits = re.findall(spec["surface_regex"], surface.read_text(encoding="utf-8"))
        print(f"[{'PASS' if hits else 'FAIL'}] live manifest: counts[{name}] "
              f"regex matches {spec['surface']} (hits={len(hits)})")
        ok &= bool(hits)
    return bool(ok)


def main() -> int:
    n = FIXTURE_FILES
    results = [
        # Agreement: the surface prints exactly what the tree holds.
        case("floor: surface number equals derived",
             sentence=f"A suite across {n}+ test files.", want_red=False),
        # The defect: the surface under-claims far beyond the staleness bound.
        case("floor: stale surface number is DRIFT",
             sentence=f"A suite across {n - 5}+ test files.", want_red=True),
        # A surface may lag by at most the declared bound.
        case("floor: lag within tolerance passes",
             sentence=f"A suite across {n - 2}+ test files.",
             surface_tolerance=3, want_red=False),
        case("floor: lag beyond tolerance is DRIFT",
             sentence=f"A suite across {n - 5}+ test files.",
             surface_tolerance=3, want_red=True),
        # Over-claiming is drift in floor mode whatever the tolerance.
        case("floor: over-claim is DRIFT",
             sentence=f"A suite across {n + 1}+ test files.",
             surface_tolerance=99, want_red=True),
        # exact mode is two-sided.
        case("exact: equal passes",
             sentence=f"A suite across {n}+ test files.",
             surface_mode="exact", want_red=False),
        case("exact: off by one is DRIFT",
             sentence=f"A suite across {n + 1}+ test files.",
             surface_mode="exact", want_red=True),
        # Vacuity: a surface that no longer prints the claim must fail CLOSED,
        # never silently stop being checked.
        case("sentence removed from surface is DRIFT",
             sentence="A suite of tests.", want_red=True),
        case("declared surface missing is DRIFT",
             sentence=f"A suite across {n}+ test files.",
             drop_surface=True, want_red=True),
        # Every printed occurrence is checked, not just the first.
        case("second stale occurrence is DRIFT",
             sentence=(f"A suite across {n}+ test files.\n\n"
                       f"Elsewhere: {n - 5}+ test files."),
             want_red=True),
        live_wiring(),
    ]
    failed = results.count(False)
    # Deliberately NOT a `ran=<n> fail=<k>` marker: scripts/gate_assert.py reads
    # that shape only from the sdlc/* family, and examples/mindc_mind/
    # smoke_wiring_lint.py refuses a hand-made one precisely because a typed
    # total is a number a gate claims rather than one it earned. Every case
    # above prints its own [PASS]/[FAIL] verdict line, which is the evidence
    # the shim counts; this summary is advisory.
    print(f"check_claims_surface_count_gate: cases={len(results)} failed={failed}")
    if failed:
        print("FAILED — the surface-count comparison is not load-bearing")
        return 1
    print("OK — every mutation of a printed count reddens the gate")
    return 0


if __name__ == "__main__":
    sys.exit(main())
