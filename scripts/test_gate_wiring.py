#!/usr/bin/env python3
"""Regression test for scripts/check_gate_wiring.py.

Guards the hole that made the attribution gates advisory-only: the gates that
keep a named-model credit out of a PUBLIC repo (file contents AND commit
messages) all live in .github/workflows/docs-claims.yml, and that workflow had
no row in .github/required-ci-jobs.tsv.  The manifest mirrors ci.yml, so the
release verifier only ever asked "is docs-claims not RED" -- and a workflow that
never ran is not red.  Deleting the commit-message step, or the manifest row
itself, therefore weakened the release contract while every gate stayed green.

Each case below MUTATES a scratch copy of the wiring and asserts the lint goes
RED.  A mutation that does not turn the lint red is a lint that is not gating
that property, so the assertion is on the exit code, never on the prose.

Run: ``python3 scripts/test_gate_wiring.py`` (no third-party deps).
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LINT = ROOT / "scripts" / "check_gate_wiring.py"


def lint_constant(name: str) -> int:
    """Read an integer constant OFF the lint source.

    Not re-typed here and not imported: a second copy of the number is the drift
    this repo keeps paying for, and importing the module would compile it into a
    __pycache__ the repo does not track.
    """
    m = re.search(rf"^{name}\s*=\s*(\d+)\s*$",
                  LINT.read_text(encoding="utf-8"), re.MULTILINE)
    if not m:
        raise SystemExit(f"test_gate_wiring: {name} not found in {LINT.name} "
                         "-- the constant moved and this test would go vacuous")
    return int(m.group(1))


HARNESS_CEILING = lint_constant("HARNESS_CEILING")

# Everything the lint reads. Copied into a scratch git repo so a mutation can be
# applied without touching the working tree.
COPIED = (
    ".github/required-ci-jobs.tsv",
    ".github/workflows/docs-claims.yml",
    ".github/workflows/ci.yml",
    ".githooks/pre-commit",
    ".githooks/commit-msg",
    ".githooks/post-commit",
    ".githooks/post-merge",
    "scripts/check_no_ai_attribution.sh",
    "scripts/check_json_not_evidence.sh",
    "scripts/check_commit_messages.sh",
    "scripts/commit-msg-hook.sh",
    "scripts/anatomy-hook.sh",
)


def scratch(dst: Path) -> None:
    """A minimal git repo holding just the wiring the lint inspects."""
    for rel in COPIED:
        src = ROOT / rel
        if not src.is_file():
            raise SystemExit(f"test_gate_wiring: missing input {rel} -- cannot build a scratch tree")
        out = dst / rel
        out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, out)
    subprocess.run(["git", "init", "-q"], cwd=dst, check=True)
    # `git ls-files` reads the INDEX, so staging is enough; no commit, no hooks.
    subprocess.run(["git", "add", "-A"], cwd=dst, check=True,
                   capture_output=True, text=True)


def run_lint(cwd: Path) -> tuple[int, str]:
    proc = subprocess.run(
        [sys.executable, str(LINT)], cwd=cwd, capture_output=True, text=True,
    )
    return proc.returncode, (proc.stdout + proc.stderr)


def drop_step(text: str, marker: str) -> str:
    """Delete the `- name:` step block whose body contains `marker`.

    Steps are the 6-space `- name:` entries of a job; the block runs to the next
    one (or to the end of the step list).
    """
    lines = text.splitlines(keepends=True)
    starts = [i for i, ln in enumerate(lines) if ln.startswith("      - name:")]
    for n, i in enumerate(starts):
        end = starts[n + 1] if n + 1 < len(starts) else len(lines)
        if marker in "".join(lines[i:end]):
            return "".join(lines[:i] + lines[end:])
    raise SystemExit(f"test_gate_wiring: no step containing {marker!r} -- mutation is vacuous")


def drop_manifest_row(text: str, needle: str) -> str:
    kept = [ln for ln in text.splitlines(keepends=True) if needle not in ln]
    if len(kept) == len(text.splitlines(keepends=True)):
        raise SystemExit(f"test_gate_wiring: no manifest row matching {needle!r} -- mutation is vacuous")
    return "".join(kept)


def main() -> int:
    failures: list[str] = []

    def case(label: str, mutate, expect_nonzero: bool,
             expect_in: str | None = None) -> None:
        """One mutation, one verdict.

        `expect_in` pins the REASON as well as the exit code. A mutation that
        breaks the scratch tree in some other way also exits non-zero, so a case
        asserting the code alone can pass while the property it names is not
        gated at all -- the shape this repo already paid for elsewhere.
        """
        with tempfile.TemporaryDirectory() as td:
            d = Path(td)
            scratch(d)
            if mutate is not None:
                mutate(d)
            rc, out = run_lint(d)
            ok = (rc != 0) if expect_nonzero else (rc == 0)
            if ok and expect_in is not None and expect_in not in out:
                ok = False
                out += f"\n(expected the failure to name: {expect_in!r})"
            verdict = "ok" if ok else "FAIL"
            print(f"  [{verdict}] {label}: exit={rc}")
            if not ok:
                failures.append(f"{label}: exit={rc}\n{out.rstrip()}")

    print("check_gate_wiring self-test")

    # Positive control. Without it, every mutation below could be passing
    # because the scratch tree is broken for an unrelated reason.
    case("unmutated scratch copy -> PASS", None, expect_nonzero=False)

    # THE mutation named by the finding: remove the commit-message gate step.
    # A commit message is the one artifact a later commit cannot correct, and
    # until this lint existed nothing noticed the step's absence.
    def del_msg_step(d: Path) -> None:
        p = d / ".github/workflows/docs-claims.yml"
        p.write_text(drop_step(p.read_text(encoding="utf-8"),
                               "scripts/check_commit_messages.sh"), encoding="utf-8")
    case("commit-message gate step deleted -> RED", del_msg_step, expect_nonzero=True)

    def del_file_gate_step(d: Path) -> None:
        p = d / ".github/workflows/docs-claims.yml"
        p.write_text(drop_step(p.read_text(encoding="utf-8"),
                               "scripts/check_no_ai_attribution.sh"), encoding="utf-8")
    case("file attribution gate step deleted -> RED", del_file_gate_step, expect_nonzero=True)

    # The finding itself: the workflow carrying those gates must be in the
    # release-required manifest, or a release can be cut from a commit where it
    # never ran.
    def del_manifest_row(d: Path) -> None:
        p = d / ".github/required-ci-jobs.tsv"
        p.write_text(drop_manifest_row(p.read_text(encoding="utf-8"), "docs-claims"),
                     encoding="utf-8")
    case("docs-claims row removed from the manifest -> RED", del_manifest_row,
         expect_nonzero=True)

    # A row whose job does not exist is a gate the release verifier will look
    # for and never find.
    def rename_job(d: Path) -> None:
        p = d / ".github/workflows/docs-claims.yml"
        p.write_text(p.read_text(encoding="utf-8").replace("\n  docs-claims:\n",
                                                           "\n  docs-claims-renamed:\n"),
                     encoding="utf-8")
    case("manifest job id absent from its workflow -> RED", rename_job, expect_nonzero=True)

    # Prose is not a gate: mentioning the script in a comment must not satisfy
    # the step requirement.
    def comment_out_step(d: Path) -> None:
        p = d / ".github/workflows/docs-claims.yml"
        txt = drop_step(p.read_text(encoding="utf-8"), "scripts/check_json_not_evidence.sh")
        txt = txt.replace("jobs:\n", "jobs:\n  # runs scripts/check_json_not_evidence.sh\n", 1)
        p.write_text(txt, encoding="utf-8")
    case("gate named only in a comment -> RED", comment_out_step, expect_nonzero=True)

    # Local defence-in-depth: the reconciled hooks directory must carry the
    # commit-msg hook, or the documented install silently drops it.
    def del_commit_msg_hook(d: Path) -> None:
        (d / ".githooks/commit-msg").unlink()
        subprocess.run(["git", "add", "-A"], cwd=d, check=True, capture_output=True)
    case(".githooks/commit-msg removed -> RED", del_commit_msg_hook, expect_nonzero=True)

    # git execs a hook; without the executable bit it is skipped in silence.
    # `core.fileMode=false` hides a local chmod from git entirely, so the bit is
    # asserted against the INDEX.
    def unset_exec_bit(d: Path) -> None:
        subprocess.run(["git", "update-index", "--chmod=-x", ".githooks/commit-msg"],
                       cwd=d, check=True, capture_output=True)
    case("hook not executable in the index -> RED", unset_exec_bit, expect_nonzero=True)

    # A hook may be a thin wrapper, but an older clone symlinks the PAYLOAD
    # script straight into the hooks directory. git resolves the symlink and
    # skips a target that is not executable, so the payload's index mode is as
    # load-bearing as the wrapper's: 100644 there is a hook that looks installed
    # and never runs.
    def unset_payload_exec_bit(d: Path) -> None:
        subprocess.run(["git", "update-index", "--chmod=-x", "scripts/commit-msg-hook.sh"],
                       cwd=d, check=True, capture_output=True)
    case("hook payload script not executable in the index -> RED",
         unset_payload_exec_bit, expect_nonzero=True)

    def del_posthooks(d: Path) -> None:
        (d / ".githooks/post-commit").unlink()
        subprocess.run(["git", "add", "-A"], cwd=d, check=True, capture_output=True)
    case(".githooks/post-commit removed -> RED", del_posthooks, expect_nonzero=True)

    # The harness ratchet. Every tracked .py/.sh here is harness -- a gate, a
    # driver, or a mutation proof for one -- and it is the file class that grows
    # without anyone deciding to grow it. The ceiling is only a ratchet if going
    # over it is RED, and only a ratchet if an empty scan is RED too.
    def overflow_harness(d: Path) -> None:
        room = d / "scratch_harness"
        room.mkdir()
        for i in range(HARNESS_CEILING + 1):
            (room / f"h{i:04d}.py").write_text("# harness\n", encoding="utf-8")
        subprocess.run(["git", "add", "-A"], cwd=d, check=True, capture_output=True)
    case(f"tracked .py/.sh over the ceiling ({HARNESS_CEILING}) -> RED",
         overflow_harness, expect_nonzero=True,
         expect_in="exceeds the recorded ceiling")

    # `0 <= ceiling` is the shape of a vacuous pass: a broken pathspec would
    # report a clean tree forever. The scratch keeps failing for its OTHER
    # missing inputs here, which is why this case pins the REASON.
    def erase_harness(d: Path) -> None:
        for f in list(d.rglob("*.py")) + list(d.rglob("*.sh")):
            f.unlink()
        subprocess.run(["git", "add", "-A"], cwd=d, check=True, capture_output=True)
    case("no tracked .py/.sh at all -> RED (not a vacuous pass)",
         erase_harness, expect_nonzero=True,
         expect_in="matched no .py/.sh at all")

    # And the real tree must pass, from the real repo root.
    rc, out = run_lint(ROOT)
    print(f"  [{'ok' if rc == 0 else 'FAIL'}] real tree -> PASS: exit={rc}")
    if rc != 0:
        failures.append(f"real tree: exit={rc}\n{out.rstrip()}")

    if failures:
        print("\ncheck_gate_wiring self-test: FAIL")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\ncheck_gate_wiring self-test: PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
