#!/usr/bin/env python3
"""gate_assert.py — the ONE definition of "did this gate actually assert anything?"

WHY THIS EXISTS
---------------
`exit 0` answers "did anything that ran fail", never "did this gate run
anything". Measured in this repo: a canary smoke that prints
`SKIP  mindc not built` and `return 0`; a claims checker that walks a corpus,
finds a different number of files than its manifest declares, and still exits 0;
a `--test` name filter that matches zero tests and reports success. Each of
those is a GREEN badge over an unasserted invariant, and a fix proven through
one is an unproven fix.

The mechanism here removes the possibility structurally rather than by asking
every gate author to remember: a gate is executed through this shim, the shim
counts the assertion evidence the gate produced, and it prints one final
machine-readable line

    asserted=<N>

`scripts/run_gate.py` treats N == 0, a missing line, or an announced skip
(`is_skip_line` below — a `SKIP`/`SKIPPED` line, or a leg a multi-leg gate
reports as `SKIPPED` mid-line) as a FAILURE. So a gate that skips its whole
body — or quietly drops one leg of several — can no longer be
indistinguishable from a gate that checked every invariant.

WHAT COUNTS AS ONE ASSERTION (one rule, defined once, here)
-----------------------------------------------------------
1. An `assert` statement that was EVALUATED. The gate's source is parsed and
   every `assert` test is wrapped so that reaching it bumps the counter. An
   `assert` inside a branch nothing takes counts zero, which is the point.
2. A VERDICT LINE the gate wrote to stdout/stderr — a completed line containing
   a standalone verdict token (PASS / FAIL / MISMATCH / ...). This is the
   corpus's existing, already-universal way of reporting a checked leg
   (`[PASS] MLIR ... main()=37 want=37`), so no gate had to be rewritten to
   start reporting, and a gate that reaches no leg reports nothing.
3. An explicit `gate_assert.bump(n)` — the escape hatch for a gate that checks
   something without printing a verdict and without using `assert`.

The repo ALREADY has machine-readable count conventions on two families of
gates: `SDLC-GATE <name> ran=<n> fail=<k>` (scripts/sdlc/*.py) and
`tcdiff ... scored=<n> divergences=<k>` (tc_differential_fuzz.py). Those markers
are the same fact under older names, so they are READ rather than
re-implemented. Two rules keep READING them from becoming a way of TYPING the
count, both paid for by a measured hole:

  * only those two LINE SHAPES are markers — anchored, with their companion
    field. A bare `ran=<n>` anywhere in any line used to be authoritative, so a
    smoke printing `(ran={len(CASES)})` published the LENGTH OF A LIST: emptying
    its case loop still reported 8, and `ran={result}(want 42)` in a diagnostic
    line reported 42 assertions for one check.
  * a marker never MANUFACTURES evidence. With zero evaluated asserts and zero
    verdict lines the count is 0 whatever the marker says, so pasting a
    sanctioned-looking line into a gate that checks nothing fails closed.

Renaming those gates' output to satisfy a new checker would have been a second
spelling of one contract — the drift shape this file exists to remove.

A gate must also never write the `asserted=` line itself: that line is this
shim's verdict about the gate, and a gate that publishes its own is either
confused or forging. Seeing one fails the run closed.

`check()` / `check_eq()` below are conveniences for NEW gates: they print one
verdict line, so they are counted by rule 2 and must NOT also bump (double
counting an assertion would inflate the only number the runner trusts).

The count is deliberately EVIDENCE-shaped, not a hand-maintained integer: a
number typed into 146 files is 146 copies of one fact and would start drifting
the day it landed — the same defect this repo keeps finding in hand-copied gate
scopes.

USAGE
  python3 scripts/gate_assert.py <gate.py> [args...]   # run a gate, counting
  from gate_assert import check, check_eq, bump        # inside a gate
"""

from __future__ import annotations

import ast
import builtins as _builtins
import io
import os
import re
import sys
from typing import Any

ASSERTED_PREFIX = "asserted="

# A completed output line carrying one of these standalone tokens is one
# reported verdict. Kept deliberately narrow to verdict vocabulary the corpus
# already uses; imported by run_gate.py so there is exactly one definition.
VERDICT_RE = re.compile(
    r"\b(?:PASS|PASSED|FAIL|FAILED|FAILURE|FAILURES|MISMATCH|DIVERGENCE)\b"
)

# A gate that announces a skipped leg has, by construction, not asserted it.
# TWO shapes, each paid for by a measured hole:
#
#   * ANCHORED  `^\s*(SKIP|SKIPPED)\b` — the announced-skip line itself. `SKIP\b`
#     alone did NOT match `SKIPPED` (`\b` needs a non-word char after the P), so
#     this rule was NARROWER than the ci.yml tee-loop backstop it supersedes
#     (`^[[:space:]]*SKIP`, unanchored suffix, which does match `SKIPPED`) — the
#     exact spelling that backstop's comment says it closed.
#   * MID-LINE  `\bSKIPPED\b` anywhere on the line — a MULTI-LEG gate announces a
#     dropped leg inside a wider sentence, which no start-of-line rule can see.
#     Measured with the oracle `.so` hidden: self_host_loop_smoke.py printed
#     `  NOTE  [ORACLE] Rust drift .so not present (...) — SKIPPED (...)`, dropped
#     its whole Rust-oracle leg, and graded `run_gate: PASS asserted=1`.
#
# Deliberately NOT mid-line `\bSKIP\b`: mindfuzz_self_host.py prints one
# `[  17] SKIP  rust rejected` line per generated CASE — a per-case
# classification inside a gate that IS asserting, not a dropped gate leg.
# Widening that far would turn a working fuzzer red, so the two shapes above are
# the whole rule and tests/gate_assert_count_contract_test.py pins both edges.
SKIP_LINE_RE = re.compile(r"^\s*(?:SKIP|SKIPPED)\b|\bSKIPPED\b")


def is_skip_line(line: str) -> bool:
    """True when this output line announces a leg the gate did not assert.

    The ONE definition — `scripts/run_gate.py` imports this rather than
    re-applying the pattern, because a `.match()` here and a `.search()` there
    is precisely how the anchored and mid-line halves would drift apart.
    """
    return SKIP_LINE_RE.search(line) is not None

# The two counter LINE SHAPES this repo already publishes for "how many did I
# actually check". Anchored and paired with their companion field on purpose: a
# bare `ran=`/`scored=` substring is a number a gate typed, not a number it
# earned, and treating one as authoritative is how a gate with an empty case
# loop reported a full count (see the module docstring).
#   scripts/sdlc/*.py             `SDLC-GATE <name> ran=<n> fail=<k>`
#   tc_differential_fuzz.py       `... scored=<n> divergences=<k>`
# examples/mindc_mind/smoke_wiring_lint.py refuses any OTHER gate source that
# prints these shapes, so the families cannot quietly grow a third member.
MARKER_RES = (
    re.compile(r"^\s*SDLC-GATE \S+ ran=(\d+) fail=\d+\b"),
    re.compile(r"\bscored=(\d+) divergences=\d+\b"),
)

# The contract line belongs to this shim. A gate printing one is forging the
# only number the runner trusts.
FORGED_RE = re.compile(r"^\s*" + re.escape(ASSERTED_PREFIX))

_count = 0
_marker_total = 0
_marker_seen = False
_forged = False


def bump(n: int = 1) -> None:
    """Record `n` assertions that produce neither an `assert` nor a verdict line."""
    global _count
    _count += int(n)


def refusal() -> str | None:
    """Why the count is being forced to zero, or None when it is not."""
    if _forged:
        return (f"the gate published its own {ASSERTED_PREFIX}line — "
                "the contract line is this shim's verdict, not the gate's")
    if _marker_seen and _count == 0:
        return ("count marker without evidence — the gate printed a count "
                "line but evaluated no assert and reported no verdict")
    return None


def count() -> int:
    """The assertion count.

    Evidence first: evaluated asserts and reported verdicts. A sanctioned count
    marker REFINES that number (those gates check more legs than they print),
    but can never conjure it from nothing.
    """
    if _forged:
        return 0
    if _marker_seen:
        return _marker_total if _count else 0
    return _count


def _ga_assert(value: Any) -> Any:
    """Injected wrapper: reaching an `assert` test is reaching an assertion."""
    global _count
    _count += 1
    return value


def check(cond: bool, label: str) -> bool:
    """Assert `cond`, reporting one verdict line (counted by the stream rule)."""
    print(f"[{'PASS' if cond else 'FAIL'}] {label}")
    return bool(cond)


def check_eq(got: Any, want: Any, label: str) -> bool:
    ok = got == want
    print(f"[{'PASS' if ok else 'FAIL'}] {label}: got={got!r} want={want!r}")
    return ok


def _count_line(line: str) -> None:
    """Fold one completed output line into the count (one rule, one place)."""
    global _count, _marker_total, _marker_seen, _forged
    if FORGED_RE.match(line):
        _forged = True
        return
    for rx in MARKER_RES:
        m = rx.search(line)
        if m:
            _marker_seen = True
            _marker_total += int(m.group(1))
            return
    if VERDICT_RE.search(line):
        _count += 1


class _CountingStream(io.TextIOBase):
    """Tee that counts completed verdict lines as they are written through."""

    def __init__(self, inner: Any) -> None:
        self._inner = inner
        self._partial = ""

    # -- io surface the gates actually use ---------------------------------
    def write(self, s: str) -> int:  # type: ignore[override]
        self._scan(s)
        return self._inner.write(s)

    def flush(self) -> None:  # type: ignore[override]
        self._inner.flush()

    def isatty(self) -> bool:  # type: ignore[override]
        return False

    def fileno(self) -> int:  # type: ignore[override]
        return self._inner.fileno()

    @property
    def encoding(self) -> str:  # type: ignore[override]
        return getattr(self._inner, "encoding", "utf-8")

    def writable(self) -> bool:  # type: ignore[override]
        return True

    # -- counting ----------------------------------------------------------
    def _scan(self, s: str) -> None:
        self._partial += s
        while "\n" in self._partial:
            line, self._partial = self._partial.split("\n", 1)
            _count_line(line)

    def close_partial(self) -> None:
        if self._partial:
            _count_line(self._partial)
        self._partial = ""


class _AssertCounter(ast.NodeTransformer):
    """Rewrites `assert T, m` to `assert __ga_assert__(T), m`."""

    def visit_Assert(self, node: ast.Assert) -> ast.Assert:
        self.generic_visit(node)
        node.test = ast.Call(
            func=ast.Name(id="__ga_assert__", ctx=ast.Load()),
            args=[node.test],
            keywords=[],
        )
        return ast.fix_missing_locations(node)


def run_script(path: str, argv: list[str]) -> int:
    """Execute a gate script with assertion counting; return its exit code."""
    src = open(path, "r", encoding="utf-8").read()
    tree = _AssertCounter().visit(ast.parse(src, filename=path))
    ast.fix_missing_locations(tree)
    code = compile(tree, filename=path, mode="exec")

    script_dir = os.path.dirname(os.path.abspath(path))
    sys.path.insert(0, script_dir)
    sys.argv = [path] + list(argv)

    g: dict[str, Any] = {
        "__name__": "__main__",
        "__file__": os.path.abspath(path),
        "__builtins__": _builtins,
        "__ga_assert__": _ga_assert,
    }
    rc = 0
    try:
        exec(code, g)
    except SystemExit as e:  # the corpus's normal exit path
        c = e.code
        rc = 0 if c is None else (c if isinstance(c, int) else 1)
    except BaseException:  # noqa: BLE001 — a crashed gate is a failed gate
        import traceback

        traceback.print_exc()
        rc = 1
    return rc


def main(argv: list[str]) -> int:
    if not argv:
        print("usage: gate_assert.py <gate.py> [args...]", file=sys.stderr)
        return 2
    path, rest = argv[0], argv[1:]
    out = _CountingStream(sys.stdout)
    err = _CountingStream(sys.stderr)
    real_out = sys.stdout
    sys.stdout, sys.stderr = out, err
    try:
        rc = run_script(path, rest)
    finally:
        out.close_partial()
        err.close_partial()
        sys.stdout, sys.stderr = real_out, sys.__stderr__
        # The contract line is emitted unconditionally — including on a crash,
        # a SystemExit from inside a helper, or an early `return 0` skip — so
        # "the line is missing" is itself a detectable, failing state rather
        # than a silent one. A forced-to-zero count says WHY on the same
        # stream, so a red run names the defect instead of only its symptom.
        why = refusal()
        if why:
            print(f"gate_assert: {why}")
        print(f"{ASSERTED_PREFIX}{count()}")
        sys.stdout.flush()
    return rc


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
