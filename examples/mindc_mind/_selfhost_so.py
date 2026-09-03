"""Shared self-host `.so` resolver for the examples/mindc_mind smokes.

Why this exists
---------------
`examples/mindc_mind/libmindc_mind.so` is a BUILD ARTIFACT — it is untracked and
gitignored (`examples/**/*.so`). Crucially, `cargo build` does NOT produce it;
only `mindc build --emit=cdylib` does. So an in-tree copy left over from an
older build goes STALE and silently false-fails the byte-identity smokes
(mic@3 flip, self-host loop, native-ELF, ...): the driver bytes drift from the
oracle even though nothing is actually wrong. CI never hits this — it builds the
`.so` fresh each run and points `MINDC_SO` at `/tmp/libmindc_mind_self_host.so`.

`resolve_so()` gives a purely-local run the same guarantee CI has:

  1. `MINDC_SO` set          -> use it verbatim (the CI contract, unchanged).
  2. else, release `mindc`   -> BUILD THE SELF-HOST `.so` FRESH via
     binary present             `mindc build --release --emit=cdylib` into a
                                per-tree temp cache (rebuilt only when `mindc`,
                                `main.mind`, `selfhost_driver.mind` or `Mind.toml`
                                change), so no stale in-tree `.so` is ever trusted.
  3. else                    -> fall back to the legacy in-tree path so each
                                smoke's existing not-found (SKIP/BLOCKED)
                                fail-closed handling fires exactly as before.

Escape hatch: `MINDC_SO_NOBUILD=1` forces the legacy in-tree default (no build) —
e.g. to point a smoke at whatever `.so` happens to be next to it on purpose.
"""

import hashlib
import os
import pathlib
import subprocess
import sys
import tempfile

_HERE = pathlib.Path(__file__).parent.resolve()
_REPO = _HERE.parents[1]
_LEGACY_SO = _HERE / "libmindc_mind.so"
_MINDC = _REPO / "target" / "release" / "mindc"

#: True once `resolve_so()` has fallen back to the legacy in-tree `.so` because a
#: fresh build was impossible. Callers that draw a CONCLUSION from the resolved
#: `.so` must qualify it: stale bytes cannot distinguish real drift from age.
USED_LEGACY_FALLBACK = False


def _stamp() -> str:
    """Fingerprint of every input that affects the emitted self-host `.so`."""
    parts = []
    # EVERY compile input, not just the entry. main.mind imports std.vec / std.map
    # / std.string / std.io, so a std/ edit changes the emitted .so while leaving
    # this stamp identical -- and _build_fresh() would then serve the CACHED .so as
    # "fresh" with USED_LEGACY_FALLBACK False, reintroducing the stale-oracle bug
    # class through the cache and defeating the very flag that guards it. The whole
    # of std/ is stamped rather than the four current imports, so adding an import
    # cannot silently narrow the fingerprint again.
    std_inputs = sorted((_REPO / "std").glob("*.mind"))
    for p in (
        _MINDC,
        _HERE / "main.mind",
        _HERE / "selfhost_driver.mind",
        _REPO / "Mind.toml",
        *std_inputs,
    ):
        try:
            st = p.stat()
            parts.append(f"{p}:{st.st_mtime_ns}:{st.st_size}")
        except OSError:
            parts.append(f"{p}:MISSING")
    return hashlib.sha256("\n".join(parts).encode()).hexdigest()


def _build_fresh() -> pathlib.Path | None:
    """Emit the self-host cdylib fresh (cached by input stamp). None on failure."""
    cache = pathlib.Path(tempfile.gettempdir()) / "mindc_mind_selfhost_cache"
    cache.mkdir(parents=True, exist_ok=True)
    so = cache / "libmindc_mind.so"
    stampf = cache / "stamp"
    want = _stamp()
    if so.exists() and stampf.is_file() and stampf.read_text() == want:
        return so
    proc = subprocess.run(
        [str(_MINDC), "build", "--release", "--emit=cdylib", f"--out={so}"],
        cwd=str(_REPO),
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0 or not so.exists():
        print(
            "WARN[_selfhost_so]: fresh `mindc build --emit=cdylib` failed "
            f"(rc={proc.returncode}) — falling back to legacy in-tree .so.\n"
            f"  stderr: {proc.stderr[-400:]}",
            file=sys.stderr,
        )
        return None
    stampf.write_text(want)
    return so


def resolve_so() -> pathlib.Path:
    """Resolve the self-host `.so` path (see module docstring for the order)."""
    # Declared once for the whole function: BOTH fallback routes below assign it,
    # and Python rejects a `global` that follows an assignment in the same scope.
    global USED_LEGACY_FALLBACK
    env = os.environ.get("MINDC_SO")
    if env:
        # Fail CLOSED on an explicit-but-missing MINDC_SO. Setting it is a promise
        # that a real `.so` is there and the gate must run for real -- it is how
        # ci.yml, fast_keystone.sh and preflight.sh all invoke these smokes. Handing
        # the path back regardless left each of ~46 importers to decide, and eight of
        # them printed `SKIP ... not built` and returned 0: a broken wiring graded as
        # a green gate. Several callers already open-code this refusal; centralising
        # it means a NEW smoke cannot reintroduce the hole by forgetting to.
        p = pathlib.Path(env)
        if not p.is_file():
            raise SystemExit(
                f"FAIL[_selfhost_so]: MINDC_SO is set to {env!r} but no such file "
                f"exists. Refusing to skip -- with MINDC_SO set, a skip would grade "
                f"a broken .so wiring as a passing gate. Build it with "
                f"`mindc build --release --emit=cdylib --out={env}`, or unset "
                f"MINDC_SO to let this resolver build a fresh one."
            )
        return p
    if os.environ.get("MINDC_SO_NOBUILD") or not _MINDC.exists():
        # Second route to the legacy artifact (explicit escape hatch, or no
        # release mindc to build with). The REASON differs from the build-failure
        # route below, but the consequence is identical: these bytes may be
        # arbitrarily old, so any caller drawing a conclusion from them must
        # qualify it. Instrumenting only one of the two routes left the flag
        # False on this path and the unqualified verdict was still printed.
        USED_LEGACY_FALLBACK = True
        return _LEGACY_SO
    fresh = _build_fresh()
    if fresh is not None:
        return fresh
    # Fell back to the legacy in-tree artifact. It WARNs above, but a caller that
    # goes on to assert a verdict from these bytes cannot tell "source drifted"
    # from "this .so is months old" -- so record the fallback and let the caller
    # qualify its conclusion. See self_host_loop_smoke.py's ORACLE leg.
    USED_LEGACY_FALLBACK = True
    return _LEGACY_SO


def resolve_mindc() -> str:
    """Resolve the `mindc` binary UNDER TEST. Fail-closed, never a PATH lookup.

    Why this exists (and why it must not consult PATH)
    --------------------------------------------------
    26 gates in this directory resolved the compiler as
    `os.environ.get("MINDC_BIN", "mindc")`. The default is a BARE NAME, so with
    no handle exported the gate ran whatever `mindc` the shell's PATH happened
    to point at — on a developer box, a symlink into a DIFFERENT checkout's
    `target/release/`. Measured: a pristine `git archive HEAD` copy with no
    `target/` at all, every handle unset, still had 21 compiler-dependent gates
    report `ALL PASS / asserted=1` — they were exercising another tree's binary,
    not the tree under test. A PATH lookup must never be able to select the
    subject under test.

    Order: `MINDC_BIN`, then `MINDC` (both already exported by ci.yml and
    preflight.sh), then this tree's `target/release/mindc`. A handle that is set
    but missing, and an unset handle with no built binary, are BOTH a hard exit —
    the gate cannot state anything about a compiler it never ran, and a SKIP here
    would grade a broken wiring as a green gate.
    """
    env = os.environ.get("MINDC_BIN") or os.environ.get("MINDC")
    p = pathlib.Path(env) if env else _MINDC
    if not p.is_file():
        raise SystemExit(
            f"FAIL[_selfhost_so]: mindc not found at {p}"
            + (" (from $MINDC_BIN/$MINDC)" if env else " (this tree's release build)")
            + ". Refusing to fall back to a PATH `mindc`: that would run another "
            "checkout's compiler and report a green gate about a tree that never "
            "built one. Build it with `cargo build --release --bin mindc`, or "
            "point $MINDC_BIN at the binary under test."
        )
    return str(p)
