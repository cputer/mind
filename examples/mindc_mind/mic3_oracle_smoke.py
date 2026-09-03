#!/usr/bin/env python3
"""mic@3 self-host convergence — Phase 0 gate: the Rust oracle.

The self-host roadmap moves the bootstrap fixed-point from lossy mic@1 TEXT
(`--emit-ir`, which flattens control-flow and stubs field-access) to the TOTAL
mic@3 binary (`--emit-mic3`). The eventual fixed point is:

    emit_mic3(pure-MIND compiler on its own source)
      ==  emit_mic3(Rust mindc on the same source)      [byte-identical]

Before the pure-MIND mic@3 emitter exists (Phases 1-5, see
mind-ecosystem-audit/SELF-HOST-MIC3-CONVERGENCE-DESIGN-2026-06-09.md), the
*oracle side* must be sound: the Rust `--emit-mic3` output must be a stable,
deterministic target. This gate proves exactly that — it is the foundation the
convergence builds against, and it RUNS and PASSES today.

Checks:
  1. `mindc <fixture> --emit-mic3` succeeds and emits a non-empty artifact.
  2. The artifact begins with the MIC3 magic + a version byte (well-formed header).
  3. Run-to-run determinism: two emits of the same source are byte-identical.

Exit 0 = PASS. No effect on the mic@1 keystone (separate emit path).

Usage:  python3 mic3_oracle_smoke.py [path/to/mindc] [path/to/fixture.mind]
"""
import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
MIC3_MAGIC = b"MIC3"


def emit_mic3(mindc: str, src: pathlib.Path) -> bytes:
    with tempfile.NamedTemporaryFile(suffix=".mic3", delete=False) as tf:
        out = pathlib.Path(tf.name)
    try:
        r = subprocess.run(
            [mindc, str(src), "--emit-mic3", str(out)],
            capture_output=True, text=True,
        )
        if r.returncode != 0:
            raise SystemExit(f"FAIL: mindc --emit-mic3 rc={r.returncode}\n{r.stderr.strip()}")
        data = out.read_bytes()
    finally:
        out.unlink(missing_ok=True)
    return data


def main() -> int:
    # argv wins, then the corpus-wide MINDC_BIN / MINDC handles, then the in-tree
    # path. Reading only target/release/mindc made this gate report success
    # against whatever binary happened to be lying in the tree — including when
    # the caller had said, explicitly, that no compiler was available.
    mindc = (sys.argv[1] if len(sys.argv) > 1
             else os.environ.get("MINDC_BIN") or os.environ.get("MINDC")
             or str(HERE.parents[1] / "target" / "release" / "mindc"))
    fixture = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else HERE / "fixture.mind"

    if not pathlib.Path(mindc).exists():
        raise SystemExit(f"FAIL: mindc not found at {mindc} (build it first)")
    if not fixture.exists():
        raise SystemExit(f"FAIL: fixture not found at {fixture}")

    print("mic@3 self-host Phase 0 — Rust oracle gate")
    print(f"  mindc:   {mindc}")
    print(f"  fixture: {fixture}")

    a = emit_mic3(mindc, fixture)
    b = emit_mic3(mindc, fixture)

    # Three separate invariants, REPORTED separately. They used to be three
    # `raise SystemExit("FAIL: ...")` guards followed by one unconditional
    # `PASS — ...` summary, so scripts/gate_assert.py could see exactly one
    # countable line no matter how many invariants had actually been evaluated —
    # the same number a gate checking nothing would publish. One verdict line per
    # invariant makes the count the work done.
    checks = [
        ("non-empty artifact", len(a) > 0, f"{len(a)} bytes"),
        ("mic@3 magic", a.startswith(MIC3_MAGIC), f"{a[:4]!r} want {MIC3_MAGIC!r}"),
        ("deterministic run-to-run", a == b,
         f"two emits of {len(a)}B/{len(b)}B "
         f"{'byte-identical' if a == b else 'DIFFER'}"),
    ]
    failed = 0
    for label, ok, detail in checks:
        print(f"  [{'PASS' if ok else 'FAIL'}] {label}: {detail}")
        failed += not ok
    if failed:
        print(f"  {failed}/{len(checks)} mic@3 oracle invariants broken")
        return 1

    version = a[len(MIC3_MAGIC)]
    digest = hashlib.sha256(a).hexdigest()
    # No verdict token on the recap — the per-invariant lines above are the count.
    print(f"  MIC3 v{version}, {len(a)} bytes, deterministic run-to-run")
    print(f"  oracle SHA-256: {digest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
