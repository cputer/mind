#!/usr/bin/env python3
"""run_gate.py — the shared gate runner that makes a VACUOUS PASS structurally fail.

THE DEFECT THIS CLOSES
----------------------
Every gate in this repo is invoked as `python3 <gate>` / `bash <gate>` and judged
by its exit code alone. An exit code answers "did anything that ran fail" — it
cannot answer "did anything run". Measured on this tree, with the compiler
binary absent:

  * examples/mindc_mind/rh_f64_aggregate_canary_smoke.py prints
    `SKIP  mindc not built` and returns 0 — a REQUIRED CI step, green, having
    compiled nothing and compared nothing.
  * a name-filtered test selector that matches zero tests reports success.
  * a corpus checker whose on-disk scan and manifest disagree still exits 0.

A fix proven through a gate like that is an unproven fix. Every gate must
therefore publish HOW MUCH it checked, and the runner — not the gate author's
memory — must refuse the answer "nothing".

THE CONTRACT (enforced here, defined in scripts/gate_assert.py)
---------------------------------------------------------------
A gate PASSES only when ALL of these hold:

  1. it exits 0;
  2. its output carries a final machine-readable `asserted=<N>` line;
  3. N >= 1 (or >= --min-asserted);
  4. no output line begins with `SKIP` — an announced skip is an unasserted
     invariant, and it must be a red gate or an explicit, recorded deferral,
     never a silent green.

Python gates get (2) for free: they are executed through `gate_assert.py`, which
counts evaluated `assert` statements and printed verdict lines and emits the
line unconditionally — including on an early `return 0` skip, where the count is
0 and the gate goes red. Non-Python gates are counted by the same verdict-line
rule applied to their output, or may print their own `asserted=` line.

USAGE
  python3 scripts/run_gate.py <gate> [args...]     # run one gate under contract
  python3 scripts/run_gate.py --sweep-all          # every gate in the corpus
  python3 scripts/run_gate.py --vacuity-sweep      # THE PROOF, see below

THE VACUITY SWEEP
-----------------
`--vacuity-sweep` runs the whole corpus with the compiler toolchain pointed at
paths that do not exist, and passes ONLY IF EVERY GATE IS REJECTED. It is the
positive control for this entire mechanism: a gate that can still report success
with no compiler present was never testing the compiler, and the sweep names it.
Being a positive control, it is the one check here that fails when it finds
NOTHING wrong with the negative case — so it cannot itself rot into a vacuous
pass.

The corpus scope is READ OUT of examples/mindc_mind/SMOKE_WIRING.tsv (the rows
already classified `gate`) rather than hand-copied into this file, because a
second hand-maintained list of the same set is the drift this repo keeps paying
for. Which of those gates the sweep expects to go RED is likewise read out of
each gate's own SOURCE — a gate is compiler-dependent iff it names a compiler
handle (MINDC / MINDC_BIN / MINDC_SO / the self-host `.so` resolver / the
release binary path). A static lint like smoke_wiring_lint.py depends on no
compiler and is CORRECT to pass without one; excluding it via a hand-written
allowlist would be a second scope to drift out of sync with the first.

MEASURED BEFORE THIS RUNNER EXISTED (the whole justification): with the compiler
binary absent, 108 of the 146 corpus gates still exited 0.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from gate_assert import ASSERTED_PREFIX, SKIP_LINE_RE, VERDICT_RE  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[1]
SMOKE_DIR = ROOT / "examples" / "mindc_mind"
MANIFEST = SMOKE_DIR / "SMOKE_WIRING.tsv"
SHIM = ROOT / "scripts" / "gate_assert.py"

DEFAULT_TIMEOUT = int(os.environ.get("MIND_GATE_TIMEOUT", "900"))
SWEEP_TIMEOUT = int(os.environ.get("MIND_GATE_SWEEP_TIMEOUT", "180"))

# The handles the sweep actually removes. A gate whose CODE names one of them
# must not be able to report success once they are gone. Matched against the
# gate's code with docstrings and comments stripped, because prose routinely
# names `mindc` in a gate that never invokes it ("Pure Python 3, stdlib only.
# No mindc build.") and a raw substring match misclassifies exactly the static
# lints it must leave alone. `'mindc'` is matched WITH its quotes so the
# directory name `examples/mindc_mind` is not mistaken for the binary.
#
# The complement is not a loophole: a gate that runs only the COMMITTED frozen
# stage1.elf (self_host_arena_growth_smoke, self_host_continue_smoke) genuinely
# does not depend on the built toolchain and is correct to pass without it, and
# so are the three pure-text lints. Naming them in a literal list here instead
# would be a second scope to drift.
TOOLCHAIN_HANDLES = ("MINDC_SO", "MINDC_BIN", "MINDC_NATIVE_ELF",
                     "'MINDC'", "'mindc'", "_selfhost_so", "resolve_so")


class Result:
    def __init__(self, gate: str, rc: int, asserted: int | None,
                 skipped: bool, reason: str, output: str) -> None:
        self.gate = gate
        self.rc = rc
        self.asserted = asserted
        self.skipped = skipped
        self.reason = reason
        self.output = output

    @property
    def ok(self) -> bool:
        return self.reason == ""


def _parse_asserted(text: str) -> int | None:
    """Last `asserted=<N>` line wins; None when the gate never published one."""
    n: int | None = None
    for line in text.splitlines():
        s = line.strip()
        if s.startswith(ASSERTED_PREFIX):
            try:
                n = int(s[len(ASSERTED_PREFIX):].strip())
            except ValueError:
                return None
    return n


def _has_skip(text: str) -> bool:
    return any(SKIP_LINE_RE.match(line) for line in text.splitlines())


def run_one(gate: str, args: list[str], *, min_asserted: int = 1,
            timeout: int = DEFAULT_TIMEOUT, env: dict[str, str] | None = None,
            echo: bool = True) -> Result:
    p = pathlib.Path(gate)
    if not p.is_absolute():
        p = (ROOT / gate) if (ROOT / gate).exists() else p
    if not p.exists():
        return Result(gate, 127, None, False,
                      f"gate not found: {gate}", "")

    if p.suffix == ".py":
        cmd = [sys.executable, str(SHIM), str(p), *args]
    elif p.suffix == ".sh":
        cmd = ["bash", str(p), *args]
    else:
        cmd = [str(p), *args]

    try:
        proc = subprocess.run(cmd, cwd=str(ROOT), capture_output=True, text=True,
                              timeout=timeout, env=env)
        out = proc.stdout + proc.stderr
        rc = proc.returncode
    except subprocess.TimeoutExpired as e:
        out = (e.stdout or "") + (e.stderr or "") if isinstance(e.stdout, str) else ""
        return Result(gate, 124, None, False, f"timed out after {timeout}s", out)

    if echo:
        sys.stdout.write(out)
        sys.stdout.flush()

    n = _parse_asserted(out)
    if n is None and p.suffix != ".py":
        # Non-Python gates are counted by the same verdict-line rule rather than
        # a second, differently-shaped mechanism.
        n = sum(1 for line in out.splitlines() if VERDICT_RE.search(line))

    skipped = _has_skip(out)
    reason = ""
    if rc != 0:
        reason = f"exit {rc}"
    elif n is None:
        reason = f"no `{ASSERTED_PREFIX}<N>` line in output (gate published no count)"
    elif n < min_asserted:
        reason = f"asserted={n} (< {min_asserted}) — the gate checked nothing"
    elif skipped:
        reason = "output contains a `SKIP` line — an unasserted invariant"
    return Result(gate, rc, n, skipped, reason, out)


def corpus() -> list[str]:
    """The gate corpus, read out of the manifest that already classifies it."""
    gates: list[str] = []
    for line in MANIFEST.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = [c.strip() for c in line.split("\t")]
        if len(parts) < 3 or parts[2] != "gate":
            continue
        name = parts[0]
        cand = SMOKE_DIR / f"{name}.py"
        if not cand.exists():
            nested = [q for q in SMOKE_DIR.rglob(f"{name}.py")
                      if "__pycache__" not in q.parts]
            if not nested:
                continue
            cand = nested[0]
        gates.append(str(cand.relative_to(ROOT)))
    return gates


def _code_without_prose(src: str) -> str:
    """The gate's executable code with docstrings (and, via unparse, comments) gone."""
    import ast

    tree = ast.parse(src)
    for node in ast.walk(tree):
        body = getattr(node, "body", None)
        if isinstance(body, list) and body and isinstance(body[0], ast.Expr) \
                and isinstance(body[0].value, ast.Constant) \
                and isinstance(body[0].value.value, str):
            body.pop(0)
    return ast.unparse(tree)


def compiler_dependent(rel: str) -> bool:
    """True iff this gate consults a toolchain handle the vacuity sweep removes."""
    try:
        src = (ROOT / rel).read_text(encoding="utf-8")
    except OSError:
        return False
    try:
        code = _code_without_prose(src)
    except SyntaxError:
        return True  # unparseable: assume it runs something, never assume it does not
    return any(m in code for m in TOOLCHAIN_HANDLES)


def _absent_toolchain_env() -> dict[str, str]:
    """Every compiler handle the corpus consults, pointed at a path that is not there."""
    env = dict(os.environ)
    missing = str(ROOT / ".gate-vacuity-sweep" / "absent")
    for var in ("MINDC", "MINDC_BIN", "MINDC_SO", "MINDC_NATIVE_ELF"):
        env[var] = missing
    env["MINDC_SO_NOBUILD"] = "1"
    return env


def sweep(expect_all_fail: bool, jobs: int, timeout: int) -> int:
    import concurrent.futures as cf

    gates = corpus()
    if not gates:
        print("run_gate: FAIL — the gate corpus is EMPTY; the manifest scope is "
              "wrong, not the corpus. Refusing to pass vacuously.")
        return 1
    env = None
    mode = "sweep-all"
    if expect_all_fail:
        env = _absent_toolchain_env()
        mode = "vacuity-sweep (toolchain absent)"
        static = [g for g in gates if not compiler_dependent(g)]
        gates = [g for g in gates if compiler_dependent(g)]
        print(f"run_gate: {len(static)} corpus gate(s) consult no compiler handle "
              f"and are correctly exempt from the absent-toolchain sweep: "
              f"{', '.join(pathlib.Path(g).name for g in sorted(static)) or '(none)'}")
        if not gates:
            print("run_gate: FAIL — no compiler-dependent gate found in the corpus; "
                  "the handle detection is wrong, not the corpus. "
                  "Refusing to pass vacuously.")
            return 1
    print(f"run_gate: {mode} over {len(gates)} gates from {MANIFEST.name}")

    with cf.ThreadPoolExecutor(max_workers=jobs) as ex:
        results = list(ex.map(
            lambda g: run_one(g, [], timeout=timeout, env=env, echo=False), gates))

    if expect_all_fail:
        leaked = [r for r in results if r.ok]
        for r in leaked:
            print(f"VACUOUS  {r.gate}: passed with NO compiler present "
                  f"(exit {r.rc}, asserted={r.asserted}) — it asserts nothing about "
                  f"the compiler")
        print(f"ran={len(results)} vacuous={len(leaked)} rejected="
              f"{len(results) - len(leaked)}")
        if leaked:
            print(f"FAIL  {len(leaked)} gate(s) pass without a compiler.")
            return 1
        print("PASS  every gate in the corpus failed closed with the toolchain absent.")
        return 0

    bad = [r for r in results if not r.ok]
    for r in bad:
        print(f"FAIL  {r.gate}: {r.reason}")
    print(f"ran={len(results)} fail={len(bad)}")
    if bad:
        return 1
    print("PASS  every gate in the corpus reported a non-zero assertion count.")
    return 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(add_help=True)
    ap.add_argument("--sweep-all", action="store_true",
                    help="run every gate in the corpus under the contract")
    ap.add_argument("--vacuity-sweep", action="store_true",
                    help="THE PROOF: run the corpus with the toolchain absent; "
                         "pass only if EVERY gate is rejected")
    ap.add_argument("--min-asserted", type=int, default=1)
    ap.add_argument("--timeout", type=int, default=0,
                    help="per-gate timeout (default: 900s single, 180s in a sweep)")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--list", action="store_true", help="print the gate corpus")
    ap.add_argument("gate", nargs="?", help="gate script to run")
    ap.add_argument("args", nargs=argparse.REMAINDER)
    ns = ap.parse_args(argv)

    if ns.list:
        for g in corpus():
            print(g)
        return 0
    if ns.vacuity_sweep:
        return sweep(True, ns.jobs, ns.timeout or SWEEP_TIMEOUT)
    if ns.sweep_all:
        return sweep(False, ns.jobs, ns.timeout or SWEEP_TIMEOUT)
    if not ns.gate:
        ap.print_usage(sys.stderr)
        return 2

    r = run_one(ns.gate, ns.args, min_asserted=ns.min_asserted,
                timeout=ns.timeout or DEFAULT_TIMEOUT)
    if r.ok:
        print(f"run_gate: PASS {ns.gate} asserted={r.asserted}")
        return 0
    print(f"run_gate: FAIL {ns.gate}: {r.reason}")
    return 1 if r.rc == 0 else r.rc


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
