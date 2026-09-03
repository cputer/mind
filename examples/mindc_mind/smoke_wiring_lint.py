#!/usr/bin/env python3
"""smoke_wiring_lint.py — machine-checked contract for WHERE each
examples/mindc_mind smoke actually runs.

WHY THIS EXISTS
---------------
The smoke corpus in this directory is the only regression gate for a large
family of constructs `main.mind` does not itself use (refs, enums, Option/Result
payloads, field stores, closures/fn-values, the float/SSE legs of the native-ELF
backend, ...).  For those, "runs in CI" is not a nicety — a regression that is
not executed by CI lands GREEN.

That contract used to be asserted in a *prose comment* on both sides
(.github/workflows/ci.yml said "fast_keystone.sh runs the SAME set ... wire it
into BOTH"; fast_keystone.sh said "wired here so CI protects each landed rung").
Prose cannot be executed, so both statements silently drifted: nine capability
gates ran ONLY in fast_keystone.sh and dozens more ran in neither runner.

This lint replaces the prose with a checked artifact.  SMOKE_WIRING.tsv declares,
for every `*.py` in this directory, which runners are expected to execute it and
why.  The lint recomputes the ACTUAL wiring by parsing the runner scripts and
fails on any divergence, in either direction:

  * a file on disk with no manifest row            -> FAIL (new smoke, unclassified)
  * a manifest row with no file on disk            -> FAIL (stale row)
  * declared runners != actual runners             -> FAIL (drift)
  * an unwired row with no stated reason           -> FAIL (silent gap)

So adding a smoke and forgetting to wire it is now a build error that forces an
explicit, reviewed decision instead of an invisible hole.

RUNNERS PARSED (the complete set that executes smokes in this repo)
  ci        .github/workflows/*.yml
  keystone  examples/mindc_mind/fast_keystone.sh
  preflight scripts/preflight.sh

Usage:
  python3 examples/mindc_mind/smoke_wiring_lint.py            # check (exit 1 on drift)
  python3 examples/mindc_mind/smoke_wiring_lint.py --print    # print the actual wiring table
  python3 examples/mindc_mind/smoke_wiring_lint.py --regen    # rewrite the runners= column from reality
                                                              # (class/note are preserved; review the diff)
"""

from __future__ import annotations

import ast
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SMOKE_DIR = ROOT / "examples" / "mindc_mind"
MANIFEST = SMOKE_DIR / "SMOKE_WIRING.tsv"

# The verdict vocabulary belongs to scripts/gate_assert.py — it is what the shim
# COUNTS. Imported, never re-spelled here: a second copy of that regex is a
# second definition of "what a reported check looks like", and the two would
# drift the first time the vocabulary grew a token, leaving this lint blind to
# exactly the lines the shim was crediting.
sys.path.insert(0, str(ROOT / "scripts"))
from gate_assert import VERDICT_RE  # noqa: E402

# runner label -> files whose text is scanned for smoke invocations
RUNNERS: dict[str, list[Path]] = {
    "ci": sorted((ROOT / ".github" / "workflows").glob("*.yml")),
    "keystone": [SMOKE_DIR / "fast_keystone.sh"],
    "preflight": [ROOT / "scripts" / "preflight.sh"],
}

# Every smoke must be classified as exactly one of these.
#   gate   a standalone regression gate; SHOULD run somewhere (unwired => must justify)
#   helper an importable module / shared fixture, never invoked directly
#   tool   an analysis or measurement utility, not a pass/fail gate
#   wip    landed-but-unfinished; unwired on purpose, with the completion condition
VALID_CLASSES = {"gate", "helper", "tool", "wip"}

NAME_RE = r"[A-Za-z0-9_]+"
# Matches a runner reference with an OPTIONAL nested directory segment. The
# on-disk scan below counts gate scripts one level deep (see the note there), so
# this must too — widening one side and not the other is precisely the
# "one rule, two sites, only one updated" defect this repo keeps finding, and it
# would report permanent DRIFT for a gate that IS correctly wired.
PATH_RE = re.compile(rf"examples/mindc_mind/(?:[A-Za-z0-9_]+/)?({NAME_RE})\.py")
# `for s in a b c ; do ... examples/mindc_mind/$s.py ... done`
LOOP_RE = re.compile(
    rf"for\s+({NAME_RE})\s+in\s+(.*?);\s*do(.*?)\bdone\b", re.DOTALL
)


def _strip_noise(text: str) -> str:
    """Drop lines that MENTION a smoke without EXECUTING it.

    Shell/YAML comments and YAML `- name:` step titles routinely quote a smoke
    filename for documentation; counting those as execution is exactly the kind
    of prose-equals-reality error this lint exists to catch.
    """
    keep = []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("#") or s.startswith("- name:") or s.startswith("name:"):
            continue
        keep.append(line)
    return "\n".join(keep)


def executed_smokes(paths: list[Path]) -> set[str]:
    found: set[str] = set()
    for p in paths:
        if not p.is_file():
            continue
        text = _strip_noise(p.read_text(encoding="utf-8"))
        found.update(PATH_RE.findall(text))
        for var, words, body in LOOP_RE.findall(text):
            if f"examples/mindc_mind/${var}.py" not in body and \
               f"examples/mindc_mind/${{{var}}}.py" not in body:
                continue
            for w in words.replace("\\", " ").split():
                if re.fullmatch(NAME_RE, w):
                    found.add(w)
    return found


def gate_sources() -> dict[str, Path]:
    """{stem: path} for every gate-shaped script this lint governs.

    The ONE definition of the on-disk scan scope. `actual_wiring()` classifies
    these names and `scan_verdict_shape_violations()` reads the same map, so the
    wiring half and the shape half of this lint cannot come to disagree about
    which files are in scope — a hand-copied second glob is the drift this file
    keeps finding elsewhere.
    """
    found: dict[str, Path] = {p.stem: p for p in sorted(SMOKE_DIR.glob("*.py"))}
    for sub in sorted(SMOKE_DIR.rglob("*.py")):
        if sub.parent == SMOKE_DIR or "__pycache__" in sub.parts:
            continue
        if sub.stem.endswith(("_smoke", "_gate", "_lint")):
            found.setdefault(sub.stem, sub)
    return found


def actual_wiring() -> dict[str, set[str]]:
    # Gate-shaped scripts NESTED below this directory count too. A `*.py` glob on
    # SMOKE_DIR alone had a blind spot exactly one level deep: the ONLY
    # cross-implementation gate for the DTK register allocator lives at
    # testdata/dtk_plan_parity_smoke.py, was referenced by no runner, and was
    # invisible to THIS lint — the check whose entire job is finding unwired gates
    # could not see it. A meta-gate with a scan blind spot is the failure mode it
    # exists to prevent, one directory deeper.
    wiring: dict[str, set[str]] = {n: set() for n in gate_sources()}
    for label, paths in RUNNERS.items():
        for name in executed_smokes(paths):
            if name in wiring:
                wiring[name].add(label)
    return wiring


# ── count-marker contract ──────────────────────────────────────────────────
# scripts/gate_assert.py publishes the ONE number scripts/run_gate.py trusts.
# It reads a count marker only in the two shapes this repo already published
# (`SDLC-GATE <name> ran=<n> fail=<k>`, `... scored=<n> divergences=<k>`), and
# never lets a marker stand in for missing evidence. Nothing stopped a gate
# author from printing a THIRD spelling by hand, which is how seven smokes came
# to publish `(ran={len(CASES)})` — the length of a list, not the number of
# cases executed — and how one diagnostic line published `ran={result}(want 42)`
# as 42 assertions. This lint is the missing half: a gate source may not print
# those shapes at all, and may never print the contract line itself.
#
# Detection is over PRINT CALLS in the parsed source, not raw text, so a
# docstring or comment describing a marker (this file, gate_assert.py,
# run_gate.py) is not mistaken for a gate emitting one.
MARKER_SCAN_DIRS = (SMOKE_DIR, ROOT / "scripts")
BARE_MARKER_RE = re.compile(r"\b(?:ran|scored)=")
SANCTIONED_RES = (
    re.compile(r"^\s*SDLC-GATE \S+ ran=(?:\{\}|\d+) fail=(?:\{\}|\d+)"),
    re.compile(r"\bscored=(?:\{\}|\d+) divergences=(?:\{\}|\d+)"),
)
FORGED_MARKER_RE = re.compile(r"^\s*asserted=")


def _template(node: ast.AST) -> str | None:
    """The literal text of a printed argument, `{}` for each interpolation."""
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    if isinstance(node, ast.JoinedStr):
        parts = []
        for v in node.values:
            if isinstance(v, ast.Constant) and isinstance(v.value, str):
                parts.append(v.value)
            else:
                parts.append("{}")
        return "".join(parts)
    return None


def _printed_templates(tree: ast.AST) -> list[str]:
    """Every string a `print(...)` / `sys.std*.write(...)` call emits."""
    out: list[str] = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        fn = node.func
        is_print = isinstance(fn, ast.Name) and fn.id == "print"
        is_write = isinstance(fn, ast.Attribute) and fn.attr == "write"
        if not (is_print or is_write):
            continue
        parts = [t for t in (_template(a) for a in node.args) if t is not None]
        if parts:
            out.append(" ".join(parts))
    return out


def marker_violations(path: Path, src: str) -> list[str]:
    """Count-marker contract violations in one gate source."""
    try:
        tree = ast.parse(src, filename=str(path))
    except SyntaxError as err:
        return [f"{path}: unparseable ({err})"]
    bad: list[str] = []
    for text in _printed_templates(tree):
        for line in text.splitlines():
            if FORGED_MARKER_RE.match(line):
                bad.append(
                    f"{path}: prints the gate_assert contract line itself "
                    f"({line.strip()!r}). That line is the shim's verdict about "
                    f"the gate; a gate printing one is forging its own count."
                )
                continue
            if not BARE_MARKER_RE.search(line):
                continue
            if any(rx.search(line) for rx in SANCTIONED_RES):
                continue
            bad.append(
                f"{path}: prints a hand-made count marker ({line.strip()!r}). "
                f"scripts/gate_assert.py reads a count only as "
                f"`SDLC-GATE <name> ran=<n> fail=<k>` or "
                f"`scored=<n> divergences=<k>`; report one verdict line per "
                f"case instead and let the shim count them."
            )
    return bad


def scan_marker_violations() -> list[str]:
    """The contract applied to every gate source under the scanned roots."""
    bad: list[str] = []
    for d in MARKER_SCAN_DIRS:
        for f in sorted(d.rglob("*.py")):
            if "__pycache__" in f.parts:
                continue
            rel = f.relative_to(ROOT)
            bad += marker_violations(rel, f.read_text(encoding="utf-8"))
    return bad


# ── per-case verdict contract ──────────────────────────────────────────
# The marker contract above stopped a PRINTED INTEGER from standing in for
# evidence. It left the other half of the same hole open: a verdict line IS
# evidence to scripts/gate_assert.py, and a single UNCONDITIONAL summary is one
# verdict line whatever the gate compared. Measured on this tree:
# self_host_array_smoke.py with `CASES = []` printed
# `ALL PASS — 0/0 byte-identical (0 diff)` and graded `run_gate: PASS
# asserted=1` — byte-identical to the unmutated 5-case control. The count could
# not tell the two apart, and the mutation every fix in this repo is required to
# survive ("comment the assertions out, expect red") could not go red.
#
# WHAT COUNTS AS SCALING EVIDENCE (the rule, stated over shape)
# -------------------------------------------------------------
# A gate that prints verdict lines at all must produce at least one piece of
# evidence that is EMITTED PER CHECKED THING on a GREEN run:
#
#   * a print whose LITERAL text carries a PASS-family token, inside a
#     For/While/AsyncFor body — one `[PASS]` per case; or
#   * an `assert` inside such a loop (the shim counts evaluated asserts, so an
#     empty loop already counts zero); or
#   * a `check()` / `check_eq()` / `bump()` call inside such a loop — the
#     gate_assert helpers, which print exactly one verdict line per call; or
#   * a runner call site that pins the gate with `--min-asserted N`, N >= 2.
#     A leg-reporting gate (the native byte-identity rungs report four legs as
#     four unconditional `[PASS]` lines) does not loop, but deleting a leg does
#     drop the count — and only a floor above 1 makes that drop fail. The floor
#     is READ OUT of the same runner scripts the wiring half parses, never
#     hand-copied here, so the two halves cannot come to disagree about which
#     call sites exist.
#
# TWO earlier spellings of this rule were measured and rejected, each because it
# graded a gate on something other than evidence:
#
#   * `If` counted as a per-case body. It is not: an `if fails: print("FAIL")`
#     recap is ONE line whatever the corpus held, and a failure-path print
#     inside a loop emits NOTHING on a green run. Measured before this was
#     tightened: self_host_tc_unknown_ident_smoke.py checked 419 cases, printed
#     one `ALL PASS`, graded `asserted=1`, and satisfied the old shape rule
#     purely through two `print("FAIL: ... drifted")` lines in a table-check
#     loop that a green run never reaches.
#   * reading string constants nested INSIDE an interpolation. That renders
#     an f-string that spells the token inside `{...}` visible, but it also
#     reads the literals of a recap line, and it flagged enum_netverify,
#     field_store_netverify and ref_netverify — three gates that DO report per
#     case — for the wording of their summary. A rule that reds correct gates to
#     reach more is a worse rule; the sanctioned way to make a composed verdict
#     visible to BOTH this scan and the shim is `gate_assert.check()`.
#
# deferred: this scan cannot see a per-case verdict whose PASS/FAIL token is
# composed at RUNTIME (`tag = "PASS" if ok else "FAIL"`, then `print(f"{tag}")`)
# — `_template` renders every interpolation as `{}`. Gates in that shape, and
# gates that still report their whole corpus with one recap line, are listed by
# name in VERDICT_SHAPE_RESIDUAL.txt rather than left as an unstated hole in the
# rule. The list is SHRINK-ONLY: a listed gate that starts satisfying the rule
# must be removed (this lint fails until it is), and a gate not on the list must
# satisfy the rule today, so the residual can only get smaller and no NEW gate
# can join it without editing a reviewed file. Upgrade path for one entry:
# route its per-case line through `gate_assert.check(cond, label)` (or add an
# `assert` in the case loop), drop the recap's verdict token, then delete its
# name here — exactly what the self_host_tc_* family did.
RESIDUAL_FILE = SMOKE_DIR / "VERDICT_SHAPE_RESIDUAL.txt"

_LOOP_NODES = (ast.For, ast.AsyncFor, ast.While)
_PASS_TOKEN_RE = re.compile(r"\b(?:PASS|PASSED)\b")
_EVIDENCE_CALLS = {"check", "check_eq", "bump"}
# `run_gate.py --min-asserted N` on a call site. N >= PINNED_FLOOR is what makes
# a leg-reporting gate's count load-bearing; the default floor of 1 only ever
# asserts "something ran".
MIN_ASSERTED_RE = re.compile(r"--min-asserted[= ]+(\d+)")
PINNED_FLOOR = 2


def pinned_floors() -> dict[str, int]:
    """{gate stem: the HIGHEST `--min-asserted` floor any runner call site pins}.

    Read out of the SAME runner scripts `actual_wiring()` parses, through the
    same noise stripping and the same loop expansion — a second hand-written
    list of call sites is the drift this file exists to catch.

    HIGHEST, not lowest: the floor's job is to make a shrunken count fail
    SOMEWHERE. A gate pinned at 4 in ci.yml and run bare in fast_keystone.sh
    still reds CI when it loses a leg, which is the property being claimed.
    """
    floors: dict[str, int] = {}

    def note(name: str, value: int) -> None:
        floors[name] = max(floors.get(name, 0), value)

    for paths in RUNNERS.values():
        for p in paths:
            if not p.is_file():
                continue
            text = _strip_noise(p.read_text(encoding="utf-8"))
            # Join shell line continuations so one invocation is one line.
            text = re.sub(r"\\\n\s*", " ", text)
            for var, words, body in LOOP_RE.findall(text):
                if f"examples/mindc_mind/${var}.py" not in body and \
                   f"examples/mindc_mind/${{{var}}}.py" not in body:
                    continue
                m = MIN_ASSERTED_RE.search(body) if "run_gate.py" in body else None
                value = int(m.group(1)) if m else 0
                for w in words.replace("\\", " ").split():
                    if re.fullmatch(NAME_RE, w):
                        note(w, value)
            for line in text.splitlines():
                if "run_gate.py" not in line:
                    continue
                m = MIN_ASSERTED_RE.search(line)
                value = int(m.group(1)) if m else 0
                for name in PATH_RE.findall(line):
                    note(name, value)
    return floors


def _verdict_evidence(tree: ast.AST) -> tuple[int, int]:
    """(verdict-bearing print calls, pieces of PER-CASE evidence in a loop)."""
    in_loop: set[int] = set()

    def descend(node: ast.AST, depth: int) -> None:
        for child in ast.iter_child_nodes(node):
            d = depth + 1 if isinstance(node, _LOOP_NODES) else depth
            if d:
                in_loop.add(id(child))
            descend(child, d)

    descend(tree, 0)

    total = evidence = 0
    for node in ast.walk(tree):
        if isinstance(node, ast.Assert) and id(node) in in_loop:
            evidence += 1
            continue
        if not isinstance(node, ast.Call):
            continue
        fn = node.func
        name = fn.id if isinstance(fn, ast.Name) else getattr(fn, "attr", "")
        if name in _EVIDENCE_CALLS and id(node) in in_loop:
            evidence += 1
            continue
        if name not in ("print", "write"):
            continue
        parts = [t for t in (_template(a) for a in node.args) if t is not None]
        if not parts:
            continue
        text = " ".join(parts)
        if not VERDICT_RE.search(text):
            continue
        total += 1
        if id(node) in in_loop and _PASS_TOKEN_RE.search(text):
            evidence += 1
    return total, evidence


def verdict_shape_violations(path: Path, src: str) -> list[str]:
    """Per-case verdict contract applied to one gate source."""
    try:
        tree = ast.parse(src, filename=str(path))
    except SyntaxError as err:
        return [f"{path}: unparseable ({err})"]
    total, evidence = _verdict_evidence(tree)
    if total == 0 or evidence > 0:
        return []
    return [
        f"{path}: prints {total} verdict line(s), none of them per checked case "
        f"— scripts/gate_assert.py reads the same asserted= count whether the "
        f"gate checked its whole corpus or an empty one. Report one PASS/FAIL "
        f"line per case inside the loop (gate_assert.check() does exactly "
        f"that), leave the summary without a verdict token, or pin the call "
        f"site with --min-asserted >= {PINNED_FLOOR}."
    ]


def load_residual() -> list[str]:
    """The declared, shrink-only list of gates this scan cannot yet bind."""
    if not RESIDUAL_FILE.is_file():
        raise SystemExit(
            f"{RESIDUAL_FILE} is missing — the verdict-shape residual is part of "
            f"the checked contract, not an optional note. Restore it (an empty "
            f"list is legal, a missing file is not)."
        )
    names = []
    for line in RESIDUAL_FILE.read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s and not s.startswith("#"):
            names.append(s)
    return names


def scan_verdict_shape_violations() -> list[str]:
    """The contract applied to every source the manifest classes as a `gate`."""
    rows, _ = parse_manifest()
    sources = gate_sources()
    floors = pinned_floors()
    residual = load_residual()
    seen = set()
    bad: list[str] = []
    for name in residual:
        if name in seen:
            bad.append(f"{RESIDUAL_FILE.name}: duplicate entry {name!r}")
        seen.add(name)
        if name not in rows or rows[name][1] != "gate":
            bad.append(
                f"{RESIDUAL_FILE.name}: {name!r} is not a `gate` row in "
                f"{MANIFEST.name}. A residual entry that names nothing hides "
                f"nothing — remove it."
            )
    for name in sorted(rows):
        if rows[name][1] != "gate":
            continue
        path = sources.get(name)
        if path is None:  # a stale/wip row; reported by the manifest checks
            continue
        if floors.get(name, 0) >= PINNED_FLOOR:
            if name in seen:
                bad.append(
                    f"{RESIDUAL_FILE.name}: {name!r} is pinned at "
                    f"--min-asserted {floors[name]} by a runner call site, so it "
                    f"is no longer residual. Delete its line."
                )
            continue
        problems = verdict_shape_violations(
            path.relative_to(ROOT), path.read_text(encoding="utf-8")
        )
        if problems and name not in seen:
            bad += problems
        if not problems and name in seen:
            bad.append(
                f"{RESIDUAL_FILE.name}: {name!r} now reports per checked case, "
                f"so it has left the residual. Delete its line — the list is "
                f"shrink-only and a stale entry would let it regress unseen."
            )
    return bad


def parse_manifest() -> tuple[dict[str, tuple[set[str], str, str]], list[str]]:
    """-> ({name: (runners, class, note)}, raw_lines)"""
    rows: dict[str, tuple[set[str], str, str]] = {}
    raw = MANIFEST.read_text(encoding="utf-8").splitlines()
    for lineno, line in enumerate(raw, 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 4:
            raise SystemExit(
                f"{MANIFEST}:{lineno}: expected 4 tab-separated columns "
                f"(smoke, runners, class, note), got {len(parts)}"
            )
        name, runners, cls, note = (p.strip() for p in parts)
        if name in rows:
            raise SystemExit(f"{MANIFEST}:{lineno}: duplicate row for {name!r}")
        decl = set() if runners == "none" else {r for r in runners.split(",") if r}
        rows[name] = (decl, cls, note)
    return rows, raw


def fmt(runners: set[str]) -> str:
    order = ["ci", "keystone", "preflight"]
    return ",".join(r for r in order if r in runners) or "none"


def main() -> int:
    argv = sys.argv[1:]
    actual = actual_wiring()

    if "--print" in argv:
        width = max(len(n) for n in actual)
        for name in sorted(actual):
            print(f"{name:<{width}}  {fmt(actual[name])}")
        print(f"\n{len(actual)} smokes; "
              f"ci={sum('ci' in v for v in actual.values())} "
              f"keystone={sum('keystone' in v for v in actual.values())} "
              f"unwired={sum(not v for v in actual.values())}")
        return 0

    # Vacuity guard. A PASS must never be reachable by finding NOTHING: a moved
    # workflows directory, a broken glob or a renamed smoke dir would otherwise
    # make "0 smokes, 0 problems" read as green — the exact false-green shape
    # this lint exists to prevent.
    if not actual:
        print("smoke_wiring_lint: FAIL — no smokes found under "
              f"{SMOKE_DIR}; the corpus path is wrong, not empty.")
        return 1
    if not any("ci" in v for v in actual.values()):
        print("smoke_wiring_lint: FAIL — no smoke is executed by any workflow in "
              f"{ROOT / '.github' / 'workflows'}; either CI wiring was deleted or "
              "the workflow parser stopped matching. Refusing to pass vacuously.")
        return 1

    rows, raw = parse_manifest()

    if "--regen" in argv:
        out = []
        for line in raw:
            if not line.strip() or line.lstrip().startswith("#"):
                out.append(line)
                continue
            name, _, cls, note = (p.strip() for p in line.split("\t"))
            out.append("\t".join([name, fmt(actual.get(name, set())), cls, note]))
        MANIFEST.write_text("\n".join(out) + "\n", encoding="utf-8")
        print(f"regenerated runners= column in {MANIFEST}")
        return 0

    errors: list[str] = []
    errors += scan_marker_violations()
    errors += scan_verdict_shape_violations()

    for name in sorted(set(actual) - set(rows)):
        errors.append(
            f"UNCLASSIFIED: examples/mindc_mind/{name}.py exists but has no row in "
            f"SMOKE_WIRING.tsv (actual runners: {fmt(actual[name])}). Add a row "
            f"declaring where it runs and why."
        )
    for name in sorted(set(rows) - set(actual)):
        # A `wip` row is allowed to name a file that is not committed yet: it
        # asserts NO coverage, it only records that an unfinished smoke exists in
        # someone's working tree. Failing on it would red a fresh checkout for a
        # file the checkout correctly does not have. Every other class must match
        # disk exactly — a `gate` row with no file is a deleted gate and stays a
        # hard error.
        if rows[name][1] == "wip":
            print(f"  note: wip row {name!r} names a file not present in this "
                  f"checkout (uncommitted work in progress) — not an error.")
            continue
        errors.append(
            f"STALE: SMOKE_WIRING.tsv lists {name!r} but "
            f"examples/mindc_mind/{name}.py does not exist. Remove the row."
        )
    for name in sorted(set(rows) & set(actual)):
        decl, cls, note = rows[name]
        if cls not in VALID_CLASSES:
            errors.append(
                f"BAD CLASS: {name}: {cls!r} not in {sorted(VALID_CLASSES)}"
            )
        if decl != actual[name]:
            errors.append(
                f"DRIFT: {name}: manifest says runners={fmt(decl)} but the runner "
                f"scripts actually execute it in {fmt(actual[name])}. Either wire it "
                f"where the manifest claims, or update the manifest."
            )
        if not actual[name] and not note:
            errors.append(
                f"SILENT GAP: {name} runs in NO runner and gives no reason. State why "
                f"in the note column (or wire it)."
            )

    if errors:
        print("smoke_wiring_lint: FAIL")
        for e in errors:
            print(f"  - {e}")
        print(f"\n{len(errors)} problem(s). "
              f"`python3 examples/mindc_mind/smoke_wiring_lint.py --print` shows the "
              f"actual wiring; `--regen` rewrites the runners= column from reality.")
        return 1

    n_ci = sum("ci" in v for v in actual.values())
    n_ks = sum("keystone" in v for v in actual.values())
    n_un = sum(not v for v in actual.values())
    print(
        f"smoke_wiring_lint: PASS — {len(actual)} smokes classified; "
        f"ci={n_ci} keystone={n_ks} unwired={n_un} (all unwired rows justified)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
