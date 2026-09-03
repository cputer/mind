#!/usr/bin/env python3
"""Self-test for the no-AI-attribution gate and for what feeds it.

WHY THIS EXISTS
---------------
A public-repo doc shipped a line naming a model as this compiler's owner, and
every gate stayed green.  Two independent holes had to line up, and each one is
invisible from inside the other:

1. ``scripts/anatomy.sh`` enumerated ``git ls-files --cached --others
   --exclude-standard`` -- i.e. TRACKED **and UNTRACKED** files.  Regenerating
   the index in a checkout that happened to hold a private, uncommitted note
   transcribed that note's title into a COMMITTED, public document.  Nothing
   about the generator's own output looks wrong; the leak is in its input set.
2. ``scripts/check_no_ai_attribution.sh`` excluded ``ANATOMY.md`` from its
   pathspec, so the one file in the tree whose contents are copied from
   arbitrary other files was also the one file the attribution gate never read.

Neither hole is testable by running the gate on the real tree and watching it
pass: a pass proves the tree is clean today, never that the gate would object
if it were not.  So each case below MUTATES a scratch copy and asserts the
gate/generator goes RED (or drops the file).  A mutation that does not change
the verdict is a property nothing is gating.

Scope values are READ OUT of the scripts under test (the scan floor, the
pathspec) rather than hand-copied here -- a hand-copied scope is the next
drift waiting to happen, which is the same defect class this file exists for.

Run: ``python3 scripts/test_no_ai_attribution.py`` (no third-party deps).
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GATE = ROOT / "scripts" / "check_no_ai_attribution.sh"
PATTERNS = ROOT / "scripts" / "ai_attribution_patterns.sh"
ANATOMY = ROOT / "scripts" / "anatomy.sh"

def rulebook(var: str) -> str:
    """Read one variable out of scripts/ai_attribution_patterns.sh.

    Sourced, never hand-copied. Two reasons, both load-bearing:
      * a hand-copied vocabulary is the drift this repo keeps paying for -- the
        self-test would keep asserting against a rule the gate no longer has;
      * this file is TRACKED and lives inside the gate's own scan scope, so a
        literal vendor name written here would make the gate red on its own
        test. Excluding this file instead would add a third never-scanned path,
        which is precisely the hole being closed. Building every vendor token at
        runtime keeps the source clean and the coverage honest.
    """
    r = subprocess.run(["bash", "-c", f'. "{PATTERNS}"; printf "%s" "${var}"'],
                       capture_output=True, text=True, check=True)
    val = r.stdout.strip()
    if not val:
        raise SystemExit(f"test_no_ai_attribution: {PATTERNS.name} defined no {var}")
    return val


# A credit shape the gate's own rulebook must already match, used as the payload
# for the in-scope cases: the first vendor token adjacent to the first credit
# noun, which is the canonical forbidden shape. These cases therefore test SCOPE
# -- "is this file read at all" -- and never pattern strength.
_NAME = rulebook("AI_NAMES").split("|")[0]
_WORD = rulebook("CREDIT_WORDS").split("|")[0]
CREDIT = f"- `notes.md` (~10 tok) - {_NAME} {_WORD} of the lowering pass"

failures: list[str] = []
checked = 0


def check(name: str, cond: bool, detail: str = "") -> None:
    """One asserted property. Prints a verdict line either way."""
    global checked
    checked += 1
    if cond:
        print(f"[PASS] {name}")
    else:
        print(f"[FAIL] {name}{': ' + detail if detail else ''}")
        failures.append(name)


def git(*args: str, cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=cwd, check=True,
                          capture_output=True, text=True)


def read_scan_floor() -> int:
    """MIN_SCANNED, read from the gate rather than duplicated here."""
    m = re.search(r"^MIN_SCANNED=(\d+)", GATE.read_text(), re.M)
    if not m:
        raise SystemExit(
            "test_no_ai_attribution: no MIN_SCANNED= in the gate -- the vacuity "
            "floor moved or was deleted; this self-test cannot build a scratch "
            "tree that clears it."
        )
    return int(m.group(1))


def scratch_gate_repo(dst: Path) -> None:
    """A git repo holding the gate, its rulebook, and enough filler to clear the
    gate's own vacuity floor (which otherwise fails every case for the wrong
    reason)."""
    (dst / "scripts").mkdir(parents=True, exist_ok=True)
    for src in (GATE, PATTERNS):
        shutil.copy2(src, dst / "scripts" / src.name)
    filler = dst / "docs"
    filler.mkdir(exist_ok=True)
    for i in range(read_scan_floor() + 1):
        (filler / f"f{i:04d}.md").write_text(f"# filler {i}\n")
    git("init", "-q", cwd=dst)
    git("add", "-A", cwd=dst)


def run_gate(repo: Path) -> subprocess.CompletedProcess:
    return subprocess.run(["bash", "scripts/check_no_ai_attribution.sh"],
                          cwd=repo, capture_output=True, text=True)


def case_generated_doc_is_in_scope() -> None:
    """ANATOMY.md is generated FROM other files, so it is the likeliest carrier
    of a credit nobody wrote by hand. Excluding it from the pathspec made it the
    one place a credit could sit in a public artifact and pass."""
    with tempfile.TemporaryDirectory() as td:
        repo = Path(td)
        scratch_gate_repo(repo)
        (repo / "ANATOMY.md").write_text(f"# Repository Anatomy\n\n{CREDIT}\n")
        git("add", "ANATOMY.md", cwd=repo)
        r = run_gate(repo)
        check("credit in generated ANATOMY.md turns the gate RED",
              r.returncode != 0,
              f"gate exited 0; stdout={r.stdout.strip()[:200]!r}")


def case_control_clean_tree_passes() -> None:
    """Positive control for the case above: without the payload the same scratch
    tree must PASS, so a RED verdict there is attributable to the credit and not
    to a scratch repo the gate simply refuses."""
    with tempfile.TemporaryDirectory() as td:
        repo = Path(td)
        scratch_gate_repo(repo)
        (repo / "ANATOMY.md").write_text("# Repository Anatomy\n\n- `notes.md` (~10 tok)\n")
        git("add", "ANATOMY.md", cwd=repo)
        r = run_gate(repo)
        check("clean generated ANATOMY.md still PASSES",
              r.returncode == 0,
              f"gate exited {r.returncode}; stderr={r.stderr.strip()[:300]!r}")


def case_anatomy_indexes_only_tracked_files() -> None:
    """The generator's INPUT set is the root cause: an uncommitted private note
    in the checkout must not be transcribable into a committed public index."""
    with tempfile.TemporaryDirectory() as td:
        repo = Path(td)
        (repo / "tracked_module.md").write_text("# Tracked Module\n")
        git("init", "-q", cwd=repo)
        git("add", "tracked_module.md", cwd=repo)
        # Untracked, not ignored -- exactly the private-handoff-note shape.
        (repo / "PRIVATE-HANDOFF.md").write_text("# Handoff for a named model (owner)\n")
        r = subprocess.run(["bash", str(ANATOMY), ".", "--output", "ANATOMY.md"],
                           cwd=repo, capture_output=True, text=True)
        out = (repo / "ANATOMY.md").read_text() if (repo / "ANATOMY.md").is_file() else ""
        check("anatomy.sh ran on the scratch repo", r.returncode == 0,
              f"rc={r.returncode} stderr={r.stderr.strip()[:300]!r}")
        check("anatomy.sh omits UNTRACKED files from the index",
              "PRIVATE-HANDOFF" not in out,
              "an uncommitted note was transcribed into the generated doc")
        # Positive control: prove the generator indexes anything at all, so the
        # assertion above cannot pass because the output was empty.
        check("anatomy.sh still indexes TRACKED files",
              "tracked_module.md" in out,
              "generator produced no index; the omission check would be vacuous")


def case_committed_anatomy_names_no_model() -> None:
    """The artifact itself, checked with a BARE-name grep over the rulebook's
    AI_NAMES -- deliberately broader than the gate's verb-adjacency PATTERN.

    This is the case that catches the leak that actually happened. The line that
    shipped read "Handoff for <model> (compiler owner)"; neither PATTERN nor
    PATTERN_CREDIT matches it, because "owner" is not credit vocabulary. The
    shared pattern is NOT the place to fix that -- naming a supported client as
    an integration target is allowed policy and appears legitimately in
    README.md and scripts/anatomy.sh, so a bare-name rule there would flag real
    documentation. The rule belongs to this ARTIFACT: a GENERATED index carries
    only filenames and first lines, and has no legitimate reason to name a
    vendor in any position. Measured when this landed: zero matches across all
    1465 indexed entries."""
    doc = ROOT / "ANATOMY.md"
    if not doc.is_file():
        check("committed ANATOMY.md exists", False, "ANATOMY.md missing")
        return
    bare = re.compile(rf"\b({rulebook('AI_NAMES')})\b", re.I)
    hits = [f"{i}: {ln.rstrip()}" for i, ln in enumerate(doc.read_text().splitlines(), 1)
            if bare.search(ln)]
    check("committed ANATOMY.md names no model", not hits, "; ".join(hits[:3]))


def case_committed_anatomy_matches_the_tracked_tree() -> None:
    """The committed index must describe THIS tree: every path it lists is
    tracked, and tracked top-level files are not missing from it."""
    doc = ROOT / "ANATOMY.md"
    tracked = set(subprocess.run(["git", "ls-files"], cwd=ROOT, check=True,
                                 capture_output=True, text=True).stdout.split())
    basenames = {Path(p).name for p in tracked}
    listed = re.findall(r"^- `([^`]+)`", doc.read_text(), re.M)
    check("ANATOMY.md lists something", bool(listed), "no entries parsed")
    unknown = sorted({n for n in listed if n not in basenames and n not in tracked})
    check("every file ANATOMY.md lists is tracked", not unknown,
          f"untracked entries: {unknown[:5]}")
    # Spot-check the other direction with files the stale index dropped.
    for rel in ("config/token_pricing.toml", ".githooks/post-merge"):
        if rel in tracked:
            check(f"ANATOMY.md lists tracked {rel}", Path(rel).name in set(listed),
                  "tracked file missing from the generated index")


def main() -> int:
    for case in (
        case_generated_doc_is_in_scope,
        case_control_clean_tree_passes,
        case_anatomy_indexes_only_tracked_files,
        case_committed_anatomy_names_no_model,
        case_committed_anatomy_matches_the_tracked_tree,
    ):
        case()
    # No `asserted=` line here on purpose: scripts/run_gate.py owns that
    # contract line and refuses a gate that publishes its own. The [PASS]/[FAIL]
    # verdict line each check() emits is what the shim counts.
    if failures:
        print(f"::error::no-ai-attribution self-test: {len(failures)} FAILED: {failures}")
        return 1
    print(f"no-ai-attribution self-test: PASS ({checked} properties)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
