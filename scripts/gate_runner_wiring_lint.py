#!/usr/bin/env python3
"""Wiring lint: a workflow may reach a gate ONLY through scripts/run_gate.py.

THE DEFECT THIS CLOSES
----------------------
scripts/run_gate.py exists because an exit code answers "did anything that ran
fail", never "did anything run": it refuses a gate that exits 0 while publishing
no `asserted=<N>` evidence, and it refuses a gate that announced a SKIP. That
guarantee is worth exactly as much as the number of call sites that go through
it. A workflow step invoking the same gate DIRECTLY is a second way to run it —
the way that cannot see a vacuous pass — and nothing noticed that the conversion
had stopped half-way.

Measured on this tree before this lint existed: 11 workflow steps invoked a gate
directly, and two of them were vacuous under the contract. `python3
tests/check_claims_cost_gate_test.py` exited 0 having reported nine mutation
cases in a verdict vocabulary (`ok `/`BAD`) the shared counter does not read, so
it published `asserted=0`; `python3 scripts/check_release_gating.py` printed a
hand-typed six-line rule list, unconditionally, whether or not those rules had
run. Both would have kept riding green with their case loops emptied.

WHAT IS CHECKED
---------------
Every invocation of a repository script (`scripts/`, `tests/`, `tools/`,
`examples/`) AND every `cargo test` / `cargo bench` step inside a workflow
`run:` block must either

  * be ROUTED — the same command also invokes scripts/run_gate.py; or
  * be EXEMPT — a comment in the same workflow declares
        `# run_gate-exempt: <path> - <reason>`
    naming that exact path (`cargo-test` / `cargo-bench` for a cargo step) with
    a written reason.

A CARGO GATE IS AN INVOCATION, AND ITS EXEMPTION IS A DEFERRAL
--------------------------------------------------------------
The scan started at repository scripts only, which left a whole class of gate
outside the mechanism: a step whose gate IS `cargo test` was not routed, not
required to be declared, and — the part that cost real integrity — not recorded
anywhere as uncovered, while this lint reported that every gate invocation
reaches the runner. Two live defects sit in exactly that blind spot: a
capability skip inside a Rust test (`println!("... skipping"); return;`) turns a
FAILING compile into a PASSING test, and the differential fuzzer reads its
program count from `MINDFUZZ_ITERS` with no floor, so a count of zero yields the
digest of zero programs on both runners — equal, non-empty, green.

Neither is reachable from the Python `asserted=N` contract, and saying so is
legitimate; letting the silence read as coverage is not. So a cargo invocation
is scanned like any other, and its exemption reason must carry a `deferred:`
marker naming the upgrade path: a cargo step publishes no `asserted=N` line
today, which makes its exemption temporary by construction. When the upgrade
lands and the direct call goes, the stale-declaration rule below removes the
deferral with it.

The exemption lives beside the call site instead of in a list inside this file:
a second, hand-maintained copy of the scope is exactly the drift this repo keeps
paying for (a lint whose scan glob and reference detection disagree). A stale
exemption naming a path no longer invoked is itself a failure, so the declared
set cannot quietly outlive the code it describes.

SCOPE DERIVATION
----------------
The workflow set is `.github/workflows/*.yml` — read off the directory, not
typed here. Within each file only `run:` blocks are scanned (`run: <cmd>` and
`run: |` block scalars, tracked by indentation), so a step NAME or a `paths:`
filter entry that merely mentions a script is not mistaken for a step that runs
one. Shell line continuations are joined first, so a command split across lines
is judged whole. A dynamic path (`"examples/mindc_mind/$s.py"`) is matched too:
a loop that invokes a gate by variable is still an invocation.

ROUTING IS A PROPERTY OF A SEGMENT, NOT OF A LINE
-------------------------------------------------
The joined command is then SPLIT on unquoted `&&`, `||`, `;` and `|`, and each
invocation is judged against its own segment: routed means THIS segment invokes
the runner with THIS path as an argument. Asking whether `scripts/run_gate.py`
appeared anywhere on the line answered yes for `python3 scripts/run_gate.py
<gate> && python3 scripts/<other gate>.py` — a routed call and a direct call
chained together, with the direct one reported as routed. That is the second way
to run a gate this lint exists to forbid, wearing the first one's evidence.

Splitting has to stop exactly there: the real tree pipes a routed gate into
`tee` inside an `if`, so the gate and its path must stay in ONE segment. A split
that broke those apart would trade a false pass for a false failure, which is
not a fix.

Dependency-free (python3 stdlib). Fails closed: finding no invocation at all is
a broken scan, not a clean tree, and is reported as a failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
WORKFLOW_DIR = ROOT / ".github" / "workflows"
RUNNER = "scripts/run_gate.py"

# A repository script referenced as a path. `$`/`{`/`}` are inside the class on
# purpose: a loop invoking `"examples/mindc_mind/$s.py"` is an invocation, and a
# pattern that could not see one would leave the loops unchecked.
PATH_RE = re.compile(
    r"(?<![A-Za-z0-9_-])(?:\./)?"
    r"((?:scripts|tests|tools|examples)/[A-Za-z0-9_./${}-]*\.(?:py|sh))"
)
EXEMPT_RE = re.compile(r"#\s*run_gate-exempt:\s*(\S+)\s*[-—:]\s*(.+?)\s*$")
MIN_REASON = 20

# A cargo gate. `cargo test`/`cargo bench` decide required CI jobs exactly like a
# script gate does, so they are invocations under the same rule. The declared
# name is the hyphenated form, because EXEMPT_RE names one token.
CARGO_RE = re.compile(r"(?<![A-Za-z0-9_./-])cargo\s+(test|bench)(?![A-Za-z0-9_-])")
CARGO_NAMES = ("cargo-test", "cargo-bench")
# A cargo step cannot publish `asserted=N`, so its exemption is a DEFERRAL that
# must name how it ends — never a standing "this one is fine".
DEFERRAL_MARKER = "deferred:"


class Invocation:
    def __init__(self, workflow: str, line: int, path: str, segment: str,
                 offset: int) -> None:
        self.workflow = workflow
        self.line = line
        self.path = path
        self.segment = segment
        self.offset = offset

    @property
    def routed(self) -> bool:
        """This invocation goes THROUGH the runner — not merely beside one.

        The runner must appear in this invocation's own segment, and the path
        must appear after it, i.e. as an argument to that runner call. `RUNNER
        in <whole line>` graded `run_gate.py a.py && python3 b.py` as two routed
        calls; b.py never met the runner.
        """
        at = self.segment.find(RUNNER)
        return at != -1 and self.offset > at

    def __str__(self) -> str:
        return f"{self.workflow}:{self.line} {self.path}"


def run_block_lines(text: str) -> list[tuple[int, str]]:
    """(1-based line number, line) for every line inside a `run:` block.

    A step name or a `paths:` filter entry naming a script is prose about a gate,
    not a step that runs one — reading the whole file would count both.
    """
    out: list[tuple[int, str]] = []
    lines = text.splitlines()
    block_indent: int | None = None
    for i, raw in enumerate(lines, start=1):
        stripped = raw.strip()
        if block_indent is not None:
            indent = len(raw) - len(raw.lstrip())
            if stripped and indent <= block_indent:
                block_indent = None
            else:
                if stripped:
                    out.append((i, raw))
                continue
        m = re.match(r"^(\s*)(?:-\s+)?run:\s*(.*)$", raw)
        if not m:
            continue
        rest = m.group(2).strip()
        if rest in ("|", ">", "|-", ">-", "|+", ">+"):
            block_indent = len(m.group(1))
        elif rest:
            out.append((i, rest))
    return out


def join_continuations(rows: list[tuple[int, str]]) -> list[tuple[int, str]]:
    """Fold shell line continuations into one logical command.

    `MINDC_SO=... \\` + `python3 scripts/run_gate.py <gate>` is ONE command; judging
    the halves separately would read the second as an unrouted invocation.
    """
    joined: list[tuple[int, str]] = []
    pending_no: int | None = None
    pending: list[str] = []
    for lineno, raw in rows:
        body = raw.strip()
        if pending_no is None:
            pending_no = lineno
        pending.append(body.rstrip("\\").strip() if body.endswith("\\") else body)
        if not body.endswith("\\"):
            joined.append((pending_no, " ".join(pending)))
            pending_no, pending = None, []
    if pending:
        joined.append((pending_no or 0, " ".join(pending)))
    return joined


def strip_shell_comment(cmd: str) -> str:
    """Drop a trailing shell comment, keeping `#` inside quotes."""
    out, quote = [], ""
    for ch in cmd:
        if quote:
            out.append(ch)
            if ch == quote:
                quote = ""
        elif ch in "'\"":
            quote = ch
            out.append(ch)
        elif ch == "#":
            break
        else:
            out.append(ch)
    return "".join(out)


SEGMENT_OPS = ("&&", "||", ";", "|")


def split_segments(cmd: str) -> list[str]:
    """Each unquoted-operator-separated part of a shell command.

    Quotes are honoured so a `&&`, `;` or `|` inside a quoted argument does not
    manufacture a segment boundary, and a backslash escape outside quotes hides
    the character it precedes. `||` is tested before `|` so a logical-or is not
    read as two empty pipes.
    """
    out: list[str] = []
    start = 0
    i = 0
    quote = ""
    n = len(cmd)
    while i < n:
        ch = cmd[i]
        if quote:
            if ch == quote:
                quote = ""
            i += 1
            continue
        if ch == "\\":
            i += 2
            continue
        if ch in "'\"":
            quote = ch
            i += 1
            continue
        for op in SEGMENT_OPS:
            if cmd.startswith(op, i):
                out.append(cmd[start:i])
                i += len(op)
                start = i
                break
        else:
            i += 1
    out.append(cmd[start:])
    return [seg for seg in out if seg.strip()]


def scan(path: Path) -> tuple[list[Invocation], dict[str, str]]:
    text = path.read_text(encoding="utf-8")
    exempt: dict[str, str] = {}
    for raw in text.splitlines():
        m = EXEMPT_RE.search(raw)
        if m:
            exempt[m.group(1)] = m.group(2)

    invocations: list[Invocation] = []
    for lineno, cmd in join_continuations(run_block_lines(text)):
        code = strip_shell_comment(cmd)
        for seg in split_segments(code):
            for hit in PATH_RE.finditer(seg):
                rel = hit.group(1)
                if rel == RUNNER:
                    continue  # the runner itself is not a gate it must route
                invocations.append(
                    Invocation(path.name, lineno, rel, seg, hit.start(1)))
            for hit in CARGO_RE.finditer(seg):
                name = f"cargo-{hit.group(1)}"
                invocations.append(
                    Invocation(path.name, lineno, name, seg, hit.start()))
    return invocations, exempt


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    # The scope is the workflow DIRECTORY, read off disk. The override exists so
    # the contract test can drive synthetic workflows through the same code path
    # the repo runs; the default is, and stays, the real tree.
    wf_dir = WORKFLOW_DIR
    if "--workflow-dir" in argv:
        i = argv.index("--workflow-dir")
        if i + 1 >= len(argv):
            print("FAIL: --workflow-dir needs a directory")
            return 1
        wf_dir = Path(argv[i + 1])

    workflows = sorted(wf_dir.glob("*.yml"))
    if not workflows:
        print(f"FAIL: no workflows under {wf_dir} — the scan found nothing "
              "to check, which is a broken lint, not a clean tree.")
        return 1

    failures: list[str] = []
    checked = 0
    routed = 0
    declared = 0
    cargo_seen = 0
    for wf in workflows:
        invocations, exempt = scan(wf)
        used: set[str] = set()
        for inv in invocations:
            checked += 1
            if inv.path in CARGO_NAMES:
                cargo_seen += 1
            if inv.routed:
                routed += 1
                print(f"[PASS] routed  {inv}")
            elif inv.path in exempt:
                used.add(inv.path)
                declared += 1
                reason = exempt[inv.path]
                if len(reason) < MIN_REASON:
                    failures.append(
                        f"{inv}: run_gate-exempt reason is {len(reason)} chars "
                        f"({reason!r}); an exemption without a written reason is "
                        "a hand-waved second way to run a gate."
                    )
                elif (inv.path in CARGO_NAMES
                      and DEFERRAL_MARKER not in reason.lower()):
                    failures.append(
                        f"{inv}: a cargo gate publishes no `asserted=N` line, so "
                        f"its exemption is a DEFERRAL and must say how it ends. "
                        f"Write `{DEFERRAL_MARKER} <upgrade path>` into the "
                        f"reason ({reason!r}); a standing exemption records an "
                        "uncovered gate as a covered one."
                    )
                else:
                    print(f"[PASS] exempt  {inv} - {reason}")
            else:
                failures.append(
                    f"{inv}: invoked DIRECTLY. An exit code cannot report a "
                    f"vacuous pass; route it as `python3 {RUNNER} {inv.path} ...`, "
                    "or declare `# run_gate-exempt: "
                    f"{inv.path} - <reason>` in {inv.workflow}."
                )
        for stale in sorted(set(exempt) - used):
            failures.append(
                f"{wf.name}: run_gate-exempt names {stale}, which this workflow "
                "does not invoke. A declaration that outlived its call site "
                "describes a tree that no longer exists."
            )

    if not checked:
        print("FAIL: scanned every workflow `run:` block and found no repository "
              "script invocation at all. The path pattern is broken, not the tree.")
        return 1

    if failures:
        print(f"\nFAIL: gate-runner wiring, {len(failures)} problem(s) "
              f"over {checked} invocation(s):")
        for f in failures:
            print(f"  - {f}")
        return 1

    print(f"\nPASS: every gate invocation in {len(workflows)} workflow(s) reaches "
          f"the gate through {RUNNER} ({routed} routed, {declared} declared "
          f"exempt, {checked} checked, of which {cargo_seen} cargo invocation(s) "
          "ride a written deferral)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
