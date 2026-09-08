#!/usr/bin/env python3
"""check_release_gating.py — machine-checked supply-chain contract for the
Release workflow.

WHY THIS EXISTS
---------------
`.github/workflows/release.yml` publishes checksum-bearing binaries for five
platforms under the project's own name.  Whatever it will build and upload is,
for every downstream user, "what MIND released".  That makes the set of
conditions under which it is *allowed to run* a security property, not a
convenience.

Before this lint, the workflow had two holes:

  1. The publishing job gated on `needs: build` alone, and the build job's whole
     step list was Checkout / Install Rust / Cache / Install cross / Build /
     Package / Upload.  No test, no keystone, no cross-substrate identity.  CI
     lives in a *different* workflow (`ci.yml`) that has no `tags:` trigger, so
     on the tag-push path no CI run is even started for that ref — the release
     could be cut from a commit whose CI was red, skipped by `paths-ignore`, or
     never run at all.
  2. `workflow_dispatch` took a free-text `version` string and then created the
     tag itself (`git tag -a v$version && git push origin v$version`) with the
     workflow-level `contents: write` token.  A dispatch on any branch therefore
     minted a brand-new tag at that branch's tip and published a non-draft
     release from it, with no requirement that the commit had ever been
     reviewed, merged, or tested.

Neither hole is visible by reading a green Actions page, and prose in a comment
cannot be executed.  This lint states both properties as assertions that fail
the build, in the same spirit as examples/mindc_mind/smoke_wiring_lint.py.

WHAT IS CHECKED
---------------
  R1  release.yml never creates or pushes a git tag.
  R2  Every job that publishes a GitHub Release transitively `needs:` the gate
      job, and the gate job actually invokes scripts/verify_ci_green.py.
  R3  workflow_dispatch takes an existing ref to release, not a free-text
      version that the workflow turns into a tag.
  R4  `contents: write` is not granted workflow-wide; only the publishing job
      may hold it (the builders and the gate run read-only).
  R5  .github/required-ci-jobs.tsv and every workflow it names agree in both
      directions, so a deleted or renamed gate cannot silently drop out of what
      a release is verified against. A row may qualify its job id as
      `<workflow file>:<job id>`; an unqualified row means ci.yml, which is why
      every pre-existing row keeps its meaning unchanged.
  R6  scripts/verify_ci_green.py exists and reads that same manifest.

Usage:
  python3 scripts/check_release_gating.py           # check (exit 1 on violation)
  python3 scripts/check_release_gating.py --print   # show what it derived
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
# The repo tracks no Python build products and .gitignore has no __pycache__
# rule; importing a sibling module would create one on every local run.
sys.dont_write_bytecode = True

from workflow_scan import (  # noqa: E402
    blocks,
    job_display_name,
    name_prefix,
    scalar_list,
    workflow_jobs,
)

ROOT = Path(__file__).resolve().parents[1]
RELEASE_YML = ROOT / ".github" / "workflows" / "release.yml"
WORKFLOW_DIR = ROOT / ".github" / "workflows"
CI_YML = WORKFLOW_DIR / "ci.yml"
# A manifest row without a `<workflow>:` qualifier names a job of this workflow.
DEFAULT_WORKFLOW = "ci.yml"
MANIFEST = ROOT / ".github" / "required-ci-jobs.tsv"
VERIFIER = ROOT / "scripts" / "verify_ci_green.py"

# Actions/commands that create a GitHub Release. A job using any of these is a
# publishing job and carries the full weight of the gate.
PUBLISH_MARKERS = (
    "softprops/action-gh-release",
    "ncipollo/release-action",
    "gh release create",
)

# Any of these appearing in a `run:` block means the workflow mints refs itself.
TAG_MINTING_RE = re.compile(
    r"git\s+tag\b|git\s+push\s+\S+\s+(?:refs/tags/|v?\$|['\"]?v[0-9])"
    r"|api\s+\S*/git/refs\b.*-X\s*POST|-X\s*POST\s+\S*/git/refs\b",
)


# The rules this gate owns, and the one-line summary each prints when it has
# ACTUALLY been evaluated. The summary used to be six unconditional `print`
# calls at the end of main(): a hand-typed list that reported all six rules as
# checked whether or not their code still existed, and published no assertion
# count at all (`asserted=0` under scripts/run_gate.py — a gate that, by the
# shared contract, checked nothing). Each rule now records that it ran, and a
# rule that records nothing is a failure, so deleting a rule's body can no
# longer leave its reassuring line behind.
RULES: dict[str, str] = {
    "R1": "release.yml mints no tags",
    "R2": "publish + build gated on the CI-green verifier",
    "R3": "dispatch takes an existing tag, not a free-text version",
    "R4": "contents:write scoped to the publishing job",
    "R5": "required-ci-jobs.tsv == every workflow it names (both directions)",
    "R6": "verify_ci_green.py present and reading that manifest",
}


def _fail(errors: list[str], rule: str, msg: str) -> None:
    errors.append(f"[{rule}] {msg}")


def _ran(ran: set[str], rule: str) -> None:
    """Record that `rule`'s body was reached and evaluated."""
    ran.add(rule)


def code(body: str) -> str:
    """The job body with comment lines dropped.

    Every rule below asks what a job DOES, and a comment saying
    "see scripts/verify_ci_green.py" is not a job that runs it. Matching on the
    raw text let a job keep its gate status by mentioning the verifier in prose
    while the `run:` line had been replaced — caught by a mutation test, so the
    stripping is applied everywhere a rule reads a body.
    """
    return "\n".join(
        ln for ln in body.splitlines() if not ln.strip().startswith("#")
    )


def parse_manifest() -> list[tuple[str, str, str]]:
    """Rows as (workflow file, job id, check-run name prefix).

    Not every release-gating job lives in ci.yml: the public-artifact hygiene
    gates run in docs-claims.yml, on that workflow's own unfiltered trigger. A
    row may therefore qualify its job id as `<workflow file>:<job id>`. An
    unqualified id still means DEFAULT_WORKFLOW, so no existing row changes.
    """
    rows: list[tuple[str, str, str]] = []
    for lineno, raw in enumerate(
        MANIFEST.read_text(encoding="utf-8").splitlines(), start=1
    ):
        line = raw.rstrip("\n")
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 2 or not parts[0].strip() or not parts[1].strip():
            raise SystemExit(
                f"{MANIFEST}:{lineno}: expected '<job id>\\t<name prefix>', got: {line!r}"
            )
        jid = parts[0].strip()
        workflow, sep, tail = jid.partition(":")
        if not sep:
            workflow, tail = DEFAULT_WORKFLOW, jid
        rows.append((workflow, tail, parts[1]))
    return rows


def check_release_workflow(errors: list[str], ran: set[str]) -> None:
    text = RELEASE_YML.read_text(encoding="utf-8")
    top = blocks(text, 0)
    jobs = workflow_jobs(RELEASE_YML)

    _ran(ran, "R1")
    # R1 — no self-minted tags anywhere in the workflow. Comment lines are
    # excluded: the file documents the human tag-push procedure in prose, and a
    # lint that cannot tell prose from a `run:` step would force the docs out.
    for jid, body in jobs.items():
        offending = [
            ln.strip() for ln in code(body).splitlines() if TAG_MINTING_RE.search(ln)
        ]
        if offending:
            _fail(
                errors,
                "R1",
                f"job '{jid}' creates or pushes a git ref: {offending}. "
                "A release must be cut from a tag that already exists on reviewed, "
                "CI-green history — the workflow must never mint the ref it publishes.",
            )

    _ran(ran, "R2")
    # R2 — publishing jobs must transitively depend on the gate job.
    gate_jobs = [jid for jid, body in jobs.items() if "verify_ci_green.py" in code(body)]
    for jid in gate_jobs:
        if "--required-jobs" in code(jobs[jid]):
            _fail(
                errors,
                "R2",
                f"gate job '{jid}' passes --required-jobs to verify_ci_green.py. That "
                "flag is a local-replay escape hatch; the shipped gate must read the "
                "checked-in .github/required-ci-jobs.tsv so the required set is the "
                "reviewed one.",
            )
    if not gate_jobs:
        _fail(
            errors,
            "R2",
            "no job in release.yml runs scripts/verify_ci_green.py. Publication "
            "must be gated on a green CI run for the exact commit being released.",
        )

    needs: dict[str, list[str]] = {
        jid: scalar_list(blocks(body, 4).get("needs", "")) for jid, body in jobs.items()
    }

    def transitive(jid: str, seen: set[str] | None = None) -> set[str]:
        seen = seen if seen is not None else set()
        for dep in needs.get(jid, []):
            if dep not in seen:
                seen.add(dep)
                transitive(dep, seen)
        return seen

    publishing = [
        jid
        for jid, body in jobs.items()
        if any(marker in code(body) for marker in PUBLISH_MARKERS)
    ]
    if not publishing:
        _fail(
            errors,
            "R2",
            "no publishing job found in release.yml — the lint cannot confirm what "
            "it is meant to gate. Update PUBLISH_MARKERS if the release action changed.",
        )
    for jid in publishing:
        deps = transitive(jid)
        if not any(g in deps or g == jid for g in gate_jobs):
            _fail(
                errors,
                "R2",
                f"publishing job '{jid}' does not depend on the CI-green gate "
                f"{gate_jobs or '(missing)'}; its dependency closure is "
                f"{sorted(deps) or '(none)'}.",
            )
    # Builders must be gated too: a build that starts before the gate wastes five
    # runners on an unverified commit and produces artifacts that look official.
    for jid, body in jobs.items():
        if jid in gate_jobs or jid in publishing:
            continue
        if "cargo build" in code(body) or "cross build" in code(body):
            deps = transitive(jid)
            if not any(g in deps for g in gate_jobs):
                _fail(
                    errors,
                    "R2",
                    f"build job '{jid}' runs before the CI-green gate "
                    f"{gate_jobs}; add it to `needs:`.",
                )

    _ran(ran, "R3")
    # R3 — dispatch input must name an existing ref, never a version to mint.
    dispatch = blocks(top.get("on", ""), 2).get("workflow_dispatch")
    if dispatch is None:
        _fail(errors, "R3", "release.yml has no workflow_dispatch trigger to check.")
    else:
        inputs = blocks(blocks(dispatch, 4).get("inputs", ""), 6)
        if "version" in inputs:
            _fail(
                errors,
                "R3",
                "workflow_dispatch still takes a free-text `version` input. The "
                "dispatch path must take an EXISTING tag, so the released commit "
                "is one that already passed review and CI.",
            )
        if "tag" not in inputs:
            _fail(
                errors,
                "R3",
                f"workflow_dispatch must take a `tag` input naming an existing tag; "
                f"found inputs {sorted(inputs) or '(none)'}.",
            )

    _ran(ran, "R4")
    # R4 — no workflow-wide write token.
    wf_perms = top.get("permissions", "")
    if re.search(r"contents:\s*write", wf_perms):
        _fail(
            errors,
            "R4",
            "workflow-level `permissions:` grants `contents: write`, so every job "
            "(including the five build runners) holds a token that can push refs. "
            "Scope write to the publishing job only.",
        )
    for jid in publishing:
        job_perms = blocks(jobs[jid], 4).get("permissions", "")
        if not re.search(r"contents:\s*write", job_perms):
            _fail(
                errors,
                "R4",
                f"publishing job '{jid}' does not declare `permissions: contents: write`; "
                "it cannot create the release with a read-only workflow token.",
            )


def check_manifest_matches_workflows(
    errors: list[str], show: bool, ran: set[str]
) -> None:
    rows = parse_manifest()
    _ran(ran, "R5")

    # One map per workflow the manifest names. Both directions are enforced for
    # EVERY such workflow, not just ci.yml: once a workflow is part of what a
    # release is verified against, a new job inside it that nobody listed is the
    # same silent gap as a new ci.yml job with no row.
    by_workflow: dict[str, dict[str, str]] = {}
    for workflow, _, _ in rows:
        if workflow in by_workflow:
            continue
        path = WORKFLOW_DIR / workflow
        if not path.is_file():
            _fail(
                errors,
                "R5",
                f"{MANIFEST.relative_to(ROOT)} names workflow {workflow!r}, which does "
                "not exist under .github/workflows/.",
            )
            by_workflow[workflow] = {}
            continue
        by_workflow[workflow] = {
            jid: name_prefix(job_display_name(body) or jid)
            for jid, body in workflow_jobs(path).items()
        }

    if show:
        for workflow, jobs in by_workflow.items():
            print(f"{workflow} jobs -> required check-run name prefix")
            for jid, prefix in jobs.items():
                print(f"  {jid:34s} {prefix!r}")
        print(f"\n{MANIFEST.relative_to(ROOT)} rows")
        for workflow, jid, prefix in rows:
            print(f"  {workflow}:{jid:34s} {prefix!r}")

    declared: dict[str, dict[str, str]] = {}
    for workflow, jid, prefix in rows:
        declared.setdefault(workflow, {})[jid] = prefix

    for workflow, jobs in by_workflow.items():
        for jid in jobs:
            if jid not in declared.get(workflow, {}):
                _fail(
                    errors,
                    "R5",
                    f"{workflow} job '{jid}' has no row in "
                    f"{MANIFEST.relative_to(ROOT)} — a new gate that no release is "
                    "verified against. Add it (or justify the gap).",
                )
        for jid, prefix in declared.get(workflow, {}).items():
            if jid not in jobs:
                _fail(
                    errors,
                    "R5",
                    f"{MANIFEST.relative_to(ROOT)} requires job '{jid}', which no longer "
                    f"exists in {workflow}. A gate was deleted or renamed; re-decide "
                    "explicitly.",
                )
            elif jobs[jid] != prefix:
                _fail(
                    errors,
                    "R5",
                    f"job '{jid}': manifest prefix {prefix!r} != {workflow} name prefix "
                    f"{jobs[jid]!r}. The gate would look for a check-run name that "
                    "is never produced.",
                )

    _ran(ran, "R6")
    # R6 — the runtime verifier exists and consumes this same manifest.
    if not VERIFIER.exists():
        _fail(errors, "R6", f"{VERIFIER.relative_to(ROOT)} is missing.")
    else:
        vtext = VERIFIER.read_text(encoding="utf-8")
        if "required-ci-jobs.tsv" not in vtext:
            _fail(
                errors,
                "R6",
                f"{VERIFIER.relative_to(ROOT)} does not reference "
                "required-ci-jobs.tsv; the static manifest and the runtime gate "
                "would be two unrelated sources of truth.",
            )


def main(argv: list[str]) -> int:
    show = "--print" in argv
    errors: list[str] = []
    ran: set[str] = set()
    check_release_workflow(errors, ran)
    check_manifest_matches_workflows(errors, show, ran)

    # A rule whose body was never reached asserted nothing, and a gate that
    # asserts nothing must not report a contract as upheld.
    for rule in RULES:
        if rule not in ran:
            _fail(
                errors,
                rule,
                "this rule's body was never evaluated — it was deleted, renamed "
                "or short-circuited. A contract nobody checked is not a contract "
                "that holds.",
            )

    if errors:
        print("release gating contract: FAIL", file=sys.stderr)
        for e in errors:
            print(f"  {e}", file=sys.stderr)
        print(
            f"\n{len(errors)} violation(s). See the module docstring in "
            "scripts/check_release_gating.py for what each rule protects.",
            file=sys.stderr,
        )
        return 1

    print("release gating contract: OK")
    # One verdict line per rule that actually ran — the evidence the shared gate
    # runner counts, rather than a fixed list printed regardless.
    for rule, summary in RULES.items():
        print(f"  [PASS] {rule} {summary}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
