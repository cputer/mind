#!/usr/bin/env python3
"""Regression test for scripts/exec_semantics_gate.sh: a FAILING test must red the tier.

THE HOLE THIS CLOSES
--------------------
The tier gate triaged failures with a sed anchored on exactly one shape:

    error: test failed, to rerun pass `--test <name>`

cargo prints `--lib`, `--doc`, `--bin <name>` and `--bench <name>` for the unit-test,
doctest, binary and benchmark harnesses, so a failing lib unit test, doctest or bin
test produced NO entry at all.  The verdict was built from that list alone, cargo's
own exit status was captured and only ever PRINTED, and the aggregate `failed` count
was printed and never asserted.  Measured on a synthetic tier log carrying a lib
harness with `3 failed`: the gate printed `failed=3` and then `ok[exec]: ... 0
failing` and exited 0.  A required release gate that cannot fail on a failing test.

WHAT THIS TEST DOES
-------------------
Each case synthesises a tier log (cheap: no cargo run, seconds not ~45 minutes),
feeds it to the real script through its ANALYSIS-ONLY `--from-log` mode, and asserts
the EXIT CODE.  Assertions are on the exit code, never on the prose, so a reworded
message cannot silently stop this test from gating.

The first case is a POSITIVE CONTROL: the same fixture with nothing wrong must exit
0.  Without it a red in every other case could just mean the fixture is malformed.
Two further cases assert the quarantine ratchet and the env-tolerated `ran=0` path
still behave exactly as before, so tightening the gate cannot be mistaken for
relaxing (or hardening) those.

Run: ``python3 scripts/test_exec_semantics_gate.py`` (no third-party deps).
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GATE = ROOT / "scripts" / "exec_semantics_gate.sh"
GATE_TEXT = GATE.read_text(encoding="utf-8") if GATE.is_file() else ""

TIERS = ("exec", "lowering", "pkg")


def _code(line: str) -> str:
    """`line` with its bash comment removed, honouring double quotes.

    A `#` only opens a comment at the start of a word and never inside `"..."`, so
    this is bash's own rule rather than `split("#", 1)`.
    """
    quoted = False
    for i, ch in enumerate(line):
        if ch == '"':
            quoted = not quoted
        elif ch == "#" and not quoted and (i == 0 or line[i - 1].isspace()):
            return line[:i]
    return line


def _array_body(name: str, text: str) -> str:
    """The code inside the bash array literal `name=( ... )`, comments removed.

    STRUCTURAL, never a non-greedy regex.  `^name=\\((.*?)\\)\\s*$` under re.S|re.M
    terminates at the first `)` that happens to end a line — including one inside a
    trailing COMMENT.  Measured on this repo: the comment `# its end-to-end leg,
    spawned for real (unix)` on the `fail_closed_capability_skip_stub_exec` row cut
    `CRITICAL_exec` to 5 of its 8 rows, so the three newest exec CRITICAL rows silently
    got no mutation case at all while the file still printed a `ran=` count.  A
    parser that can SHRINK a mutation matrix without saying so is the same fail-open
    shape this gate exists to remove, so the scan walks lines and closes on the first
    `)` that is real code — exactly where bash closes the literal.
    """
    m = re.search(rf"^{re.escape(name)}=\(", text, re.M)
    if m is None:
        raise SystemExit(f"FAIL: {GATE.name} has no {name}=( ... ) array")
    body: list[str] = []
    for line in text[m.end() :].split("\n"):
        stripped = _code(line).strip()
        if stripped.endswith(")"):
            body.append(stripped[:-1])
            return "\n".join(body)
        body.append(stripped)
    raise SystemExit(f"FAIL: {GATE.name}: {name}=( is never closed by a `)`")


def _array(name: str, text: str | None = None) -> tuple[str, ...]:
    """Entries of the bash array literal `name=( ... )` in the gate script."""
    body = _array_body(name, GATE_TEXT if text is None else text)
    if body.count('"') % 2:
        raise SystemExit(f"FAIL: {GATE.name}: {name}=( ... ) has an unbalanced quote")
    entries = tuple(tok.strip('"') for tok in re.findall(r'"[^"]*"|\S+', body))
    # SELF-CHECK: the tokeniser must recover every quoted row of the block it was
    # handed.  A parse that silently returns FEWER entries than the literal contains
    # shrinks the generated mutation matrix, and a shrunken matrix still prints a
    # plausible `ran=` — so a mismatch is loud and fatal, never a smaller number.
    quoted = len(re.findall(r'"[^"]*"', body))
    if quoted and quoted != len(entries):
        raise SystemExit(
            f"FAIL: {GATE.name}: {name}=( ... ) parsed {len(entries)} entries from "
            f"{quoted} quoted rows -- the array parse is truncating"
        )
    return entries


def _int(name: str) -> int:
    m = re.search(rf"^{name}=([0-9]+)$", GATE_TEXT, re.M)
    if m is None:
        raise SystemExit(f"FAIL: {GATE.name} has no {name}=<integer>")
    return int(m.group(1))


# READ OUT OF THE SCRIPT, never hand-copied.  A hand-written mirror is the same
# drift shape this whole gate exists to remove: the moment a CRITICAL row or a floor
# moves in the script, a stale fixture stops satisfying it and the failure reads as a
# gate defect instead of a test-fixture defect.  `CRITICAL[tier]` is the parsed
# `(target, minimum)` pairs; `QUARANTINED[tier]` the targets that must keep FAILING
# in a baseline fixture, or the shrink-only ratchet reds the tier itself.
CRITICAL = {
    t: tuple((e.split()[0], int(e.split()[1])) for e in _array(f"CRITICAL_{t}")) for t in TIERS
}
QUARANTINED = {t: _array(f"QUARANTINE_{t}") for t in TIERS}
# Read out of the gate script itself, never hand-copied: the TOLERANCE
# SHRINK-RATCHET fails a tier whose ENV_TOLERATED list names a target the tier
# never built, so a faithful fixture must RUN every tolerated target.
TOLERATED = {t: _array(f"ENV_TOLERATED_{t}") for t in TIERS}
FLOOR_TESTS = {t: _int(f"FLOOR_TESTS_{t}") for t in TIERS}
FLOOR_HARNESSES = {t: _int(f"FLOOR_HARNESSES_{t}") for t in TIERS}

# Sized from the gate's own numbers so the fixture clears every floor with a wide
# margin whatever they are today — and so PER_HARNESS always satisfies the largest
# CRITICAL minimum, which is what makes the erasure cases below isolate the
# per-harness check from the aggregate floor.
CRITICAL_NAMES = {t: [n for n, _ in rows] for t, rows in CRITICAL.items()}
HARNESSES = max(FLOOR_HARNESSES.values()) + 20
PER_HARNESS = max(10, *(m for rows in CRITICAL.values() for _, m in rows))


def result_line(passed: int, failed: int = 0, ignored: int = 0) -> str:
    status = "FAILED" if failed else "ok"
    return (
        f"test result: {status}. {passed} passed; {failed} failed; "
        f"{ignored} ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
    )


def block(header: str, passed: int, failed: int = 0, ignored: int = 0, body: str = "") -> str:
    total = passed + failed + ignored
    return (
        f"     {header}\n"
        f"\nrunning {total} tests\n"
        f"{body}"
        f"{result_line(passed, failed, ignored)}\n"
    )


def tier_log(
    tier: str,
    *,
    extra_blocks: str = "",
    error_lines: tuple[str, ...] = (),
    cargo_exit: int = 0,
    emit_exit_marker: bool = True,
    erase: tuple[str, ...] = (),
    omit_tolerated: bool = False,
) -> str:
    """A log that clears every floor of `tier`, plus whatever the case injects.

    `erase` reproduces what cargo prints for a test file that was DELETED or cfg'd
    out: the harness is still counted and still reports `ok`, with 0 tests.
    """
    out = ["   Compiling libmind v0.1.0 (/w)\n    Finished test profile\n"]
    for name, _minimum in CRITICAL[tier]:
        ran = 0 if name in erase else PER_HARNESS
        out.append(block(f"Running tests/{name}.rs (target/debug/deps/{name}-01)", ran))
    for name in QUARANTINED[tier]:
        # Quarantined targets must stay RED or the ratchet reds the tier itself.
        out.append(
            block(f"Running tests/{name}.rs (target/debug/deps/{name}-02)", PER_HARNESS - 1, 1)
        )
        error_lines = error_lines + (f"error: test failed, to rerun pass `--test {name}`",)
    tolerated = (
        []
        if omit_tolerated
        else [n for n in TOLERATED[tier] if n not in CRITICAL_NAMES[tier] and n not in QUARANTINED[tier]]
    )
    for name in tolerated:
        # An env-tolerated target that the tier BUILDS and that passes. Tolerance
        # is for its ran=0, not for its absence.
        out.append(
            block(f"Running tests/{name}.rs (target/debug/deps/{name}-04)", PER_HARNESS)
        )
    filler = HARNESSES - len(CRITICAL[tier]) - len(QUARANTINED[tier]) - len(tolerated)
    for i in range(filler):
        out.append(
            block(f"Running tests/filler_{i:03d}.rs (target/debug/deps/filler_{i:03d}-03)", PER_HARNESS)
        )
    out.append(extra_blocks)
    for line in error_lines:
        out.append(line + "\n")
    if emit_exit_marker:
        out.append(f"MIND_TIER_CARGO_EXIT={cargo_exit}\n")
    return "".join(out)


def run_gate(log_text: str, tier: str) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as td:
        log = Path(td) / f"mind-tier-{tier}.log"
        log.write_text(log_text, encoding="utf-8")
        return subprocess.run(
            ["bash", str(GATE), "--from-log", str(log), tier],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )


CASES: list[tuple[str, str, str, int]] = []


def case(name: str, tier: str, log_text: str, want_nonzero: bool) -> None:
    CASES.append((name, tier, log_text, 1 if want_nonzero else 0))


# --- 0. POSITIVE CONTROL -----------------------------------------------------
case("baseline exec log is GREEN (fixture sanity)", "exec", tier_log("exec"), False)

# --- 0b. TOLERANCE SHRINK-RATCHET -------------------------------------------
# The missing half of the quarantine ratchet. A quarantined target that starts
# passing must leave its list; an ENV_TOLERATED name the tier never BUILDS had no
# such rule and produced no signal at all, so dead tolerance accumulated silently
# and pre-approved a ran=0 for the day the target became buildable again.
if TOLERATED["exec"]:
    case(
        "an ENV_TOLERATED target this tier never built reds 'exec'",
        "exec",
        tier_log("exec", omit_tolerated=True),
        True,
    )

# --- 1..3. the three harness kinds cargo names with a non---test selector ----
case(
    "a failing lib UNIT test reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block(
            "Running unittests src/lib.rs (target/debug/deps/libmind-a1)",
            100,
            3,
            body="test ir::compact::v3::tests::roundtrip ... FAILED\n",
        ),
        error_lines=("error: test failed, to rerun pass `--lib`",),
        cargo_exit=101,
    ),
    True,
)
case(
    "a failing DOCTEST reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block("Doc-tests libmind", 40, 1),
        error_lines=("error: test failed, to rerun pass `--doc`",),
        cargo_exit=101,
    ),
    True,
)
case(
    "a failing BIN test reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block("Running unittests src/main.rs (target/debug/deps/mindc-b2)", 9, 2),
        error_lines=("error: test failed, to rerun pass `--bin mindc`",),
        cargo_exit=101,
    ),
    True,
)

# --- 4. cargo failed and NOTHING in the log explains it ----------------------
# Every harness reports ok and no rerun hint is printed, yet cargo exited 101 (a
# link failure, a harness that aborted before printing, a future cargo whose
# wording changed).  Fail closed: an unexplained non-zero exit is a failure.
case(
    "an UNATTRIBUTED non-zero cargo exit reds the tier",
    "exec",
    tier_log("exec", cargo_exit=101),
    True,
)

# --- 5. the aggregate failed count is asserted, not decorative ---------------
# No `error: test failed` line at all -- the shape the triage sed depends on is
# gone -- but a harness reported `2 failed`.  The count alone must red the tier,
# so the gate does not rest on one grep of one cargo message.
case(
    "a harness reporting failed>0 with NO rerun hint still reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block("Running tests/some_gate.rs (target/debug/deps/some_gate-c3)", 4, 2),
        cargo_exit=101,
    ),
    True,
)

# --- 6. the quarantine ratchet is preserved exactly --------------------------
# lowering's baseline log fails std_surface_intrinsics (a QUARANTINE_lowering
# entry) with a non-zero cargo exit.  That is the shape the gate is green on
# today and must stay green on: tightening must not turn the ratchet into a wall.
case(
    "a QUARANTINED failing target keeps the tier green",
    "lowering",
    tier_log("lowering", cargo_exit=101),
    False,
)

# --- 7. env-tolerated ran=0 is preserved exactly -----------------------------
case(
    "an ENV_TOLERATED ran=0 skip keeps the tier green",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block(
            "Running tests/g2_differential_mlir.rs (target/debug/deps/g2_differential_mlir-d4)",
            1,
            body="SDLC-GATE g2_differential_mlir ran=0 fail=0\n",
        ),
    ),
    False,
)

# --- 8. a NON-tolerated ran=0 still reds the tier ----------------------------
case(
    "a non-tolerated ran=0 marker still reds the tier",
    "exec",
    tier_log(
        "exec",
        extra_blocks=block(
            "Running tests/some_gate.rs (target/debug/deps/some_gate-e5)",
            1,
            body="SDLC-GATE some_gate ran=0 fail=0\n",
        ),
    ),
    True,
)


# --- 9..N. EVERY CRITICAL row must BITE --------------------------------------
# One case per row, generated from the script's own lists, so a row added later is
# proven to gate without anyone remembering to write its test — and a row that is
# only decorative can never be added unnoticed.
#
# These cases isolate the per-harness minimum from the aggregate floors: the fixture
# clears both floors by a wide margin and loses only PER_HARNESS tests to the
# erasure, so the ONLY check that can red the tier is the CRITICAL one.  That is the
# hole the mechanism exists to close — an erased file still prints
# `test result: ok. 0 passed` and leaves the HARNESS count unmoved, so a tier with
# slack absorbs the whole file in silence.
for _tier in TIERS:
    for _name, _minimum in CRITICAL[_tier]:
        case(
            f"an ERASED critical harness reds '{_tier}' ({_name}, min {_minimum})",
            _tier,
            tier_log(_tier, erase=(_name,)),
            True,
        )


# --- P1..P5. the ARRAY PARSER itself, which sizes every case above ----------
# The generated cases are only as complete as the parse of `CRITICAL_<tier>`, so a
# truncating parse is invisible: fewer cases still print a `ran=` and every one of
# them passes.  These checks pin the parse against the exact shapes the gate script
# uses, including the trailing-comment-ends-in-`)` row that truncated it for real.
_PARSER_FIXTURE = """\
DECOY=(should_not_be_read)
SAMPLE=(
  "alpha 2"       # its end-to-end leg, spawned for real (unix)
  "beta 1"        # another ) in a comment
  "gamma 3"
)
ONE_LINE=(solo)
INLINE=(one two three)
EMPTY=(
)
"""


def _quoted_rows_between(start: str, end: str) -> int:
    """Quoted rows in the gate script between two markers, comments removed.

    Independent of the CLOSING rule in `_array_body` — which is the rule that
    truncated — so it is a real cross-check on how far the parse reached, and a
    double quote inside a comment cannot false-red it.
    """
    region = GATE_TEXT[GATE_TEXT.index(start) : GATE_TEXT.index(end)]
    code = "\n".join(_code(line) for line in region.split("\n"))
    return len(re.findall(r'"[^"]*"', code))


PARSER_CHECKS: list[tuple[str, Callable[[], object], object]] = [
    (
        "a CRITICAL row whose comment ends in ')' is still parsed",
        lambda: _array("SAMPLE", _PARSER_FIXTURE),
        ("alpha 2", "beta 1", "gamma 3"),
    ),
    (
        "a single-line array literal parses",
        lambda: _array("ONE_LINE", _PARSER_FIXTURE),
        ("solo",),
    ),
    (
        "a single-line array with several entries parses",
        lambda: _array("INLINE", _PARSER_FIXTURE),
        ("one", "two", "three"),
    ),
    (
        "an empty array literal parses as empty",
        lambda: _array("EMPTY", _PARSER_FIXTURE),
        (),
    ),
    (
        "every quoted CRITICAL_exec row in the gate script is generated",
        lambda: len(CRITICAL["exec"]),
        _quoted_rows_between("CRITICAL_exec=(", "CRITICAL_lowering="),
    ),
]


def main() -> int:
    if not GATE.is_file():
        print(f"FAIL: {GATE} not found", file=sys.stderr)
        return 2
    bad = 0
    for name, produce, want in PARSER_CHECKS:
        got = produce()
        ok = got == want
        print(f"[{'PASS' if ok else 'FAIL'}] {name}: want {want!r}, got {got!r}")
        if not ok:
            bad += 1
    for name, tier, log_text, want_nonzero in CASES:
        proc = run_gate(log_text, tier)
        got_nonzero = 1 if proc.returncode != 0 else 0
        ok = got_nonzero == want_nonzero
        # PASS, not `ok`: scripts/gate_assert.py counts a reported verdict
        # from a fixed token set, and `[ok]` is outside it -- every case here
        # published NO evidence, so the count marker below had nothing to
        # refine and the gate graded `asserted=0` once routed.
        verdict = "PASS" if ok else "FAIL"
        want = "non-zero" if want_nonzero else "0"
        print(f"[{verdict}] {name}: want exit {want}, got {proc.returncode}")
        if not ok:
            bad += 1
            sys.stdout.write(proc.stdout[-2500:])
            sys.stderr.write(proc.stderr[-2000:])
    # The sanctioned marker shape (see scripts/gate_assert.py MARKER_RES); a
    # bare `ran=/fail=` is refused by examples/mindc_mind/smoke_wiring_lint.py.
    ran = len(CASES) + len(PARSER_CHECKS)
    print(f"\nSDLC-GATE exec_semantics_gate_selftest ran={ran} fail={bad}")
    if bad:
        print("FAIL: exec_semantics_gate.sh did not grade as required above")
        return 1
    print("OK: a failing lib/doc/bin test, an unattributed cargo exit and a bare")
    print("    failed>0 count each red the tier; quarantine and env-tolerance intact")
    return 0


if __name__ == "__main__":
    sys.exit(main())
