#!/usr/bin/env python3
"""Gate: `resolve_so()` must QUALIFY the `.so` it returns and REFUSE a stale one.

Why this gate exists
--------------------
`examples/mindc_mind/libmindc_mind.so` is an untracked build artifact that
`cargo build` does not regenerate, so an in-tree copy goes arbitrarily old. The
stale-oracle qualification used to live in ONE of ~47 `resolve_so()` importers
(`self_host_loop_smoke.py` consulted the `USED_LEGACY_FALLBACK` flag; nobody
else did), and the `MINDC_SO` route set no flag at all — so pointing `MINDC_SO`
at the legacy artifact laundered it as a promised real oracle and every sibling
byte-identity smoke certified `ALL PASS` on a compiler that the loop gate, on
the very same bytes, refuses.

The contract this gate pins, at the resolver rather than at 47 call sites:

  * every resolved path CARRIES its provenance (fresh-build / env-verified /
    env-stale / legacy-in-tree);
  * `resolve_so()` REFUSES a non-fresh oracle by default — a caller does not
    have to remember to check;
  * an age-tolerant caller must opt out EXPLICITLY and NAME its reason;
  * fail-closed on an absent `MINDC_SO` is preserved (a set-but-missing
    `MINDC_SO` must never degrade to a skip).

Pure stdlib: no toolchain, no build, no `.so` needed. Reports one verdict line
per assertion and closes with `SDLC-GATE selfhost_so_provenance ran=<n> fail=<k>`
so a zero-assertion run cannot read as a pass.
"""

import os
import pathlib
import re
import sys
import tempfile
import time

_HERE = pathlib.Path(__file__).parent.resolve()
sys.path.insert(0, str(_HERE))

import _selfhost_so as mod  # noqa: E402

RAN = 0
FAILED: list[str] = []

#: Files allowed to resolve a stale `.so` on purpose. Only this gate is listed —
#: it must exercise the opt-out route to prove the route works. Any other entry
#: must arrive with the reason it names in its own `resolve_so()` call, so a
#: smoke cannot quietly step out of the freshness contract.
ALLOWED_OPTOUTS: set[str] = {"selfhost_so_provenance_smoke.py"}


def check(name: str, cond: bool, detail: str = "") -> None:
    """One assertion, ONE verdict line -- in the vocabulary the shim counts.

    `  ok   <name>` is not a token scripts/gate_assert.py reads, so all sixteen
    assertions below published NO evidence: routed through scripts/run_gate.py this
    gate graded `asserted=1` (its single closing PASS), and deleting fifteen of the
    sixteen checks would have left that number, and the exit code, untouched.
    `[PASS]`/`[FAIL]` per check makes the count scale with the work actually done.
    """
    global RAN
    RAN += 1
    if cond:
        print(f"  [PASS] {name}")
    else:
        print(f"  [FAIL] {name}{(' — ' + detail) if detail else ''}")
        FAILED.append(name)


def _clear_env() -> None:
    for k in ("MINDC_SO", "MINDC_SO_NOBUILD"):
        os.environ.pop(k, None)


def _call(**kw):
    """Return ('ok', path) or ('refused', message)."""
    try:
        return ("ok", mod.resolve_so(**kw))
    except SystemExit as e:
        return ("refused", str(e))


def main() -> int:
    print("=== resolve_so() stale-oracle provenance gate ===")
    tmp = pathlib.Path(tempfile.mkdtemp(prefix="so_provenance_"))

    # ---- 1. an OLD MINDC_SO is refused ---------------------------------
    stale = tmp / "stale.so"
    stale.write_bytes(b"\x7fELF stale")
    old = time.time() - 400 * 24 * 3600
    os.utime(stale, (old, old))
    _clear_env()
    os.environ["MINDC_SO"] = str(stale)
    kind, res = _call()
    check(
        "MINDC_SO pointing at an artifact older than the sources is REFUSED",
        kind == "refused",
        f"got {kind}: {res}",
    )
    if kind == "refused":
        check(
            "the refusal names the provenance and the fix",
            "stale" in res.lower() and "MINDC_SO" in res,
            res[:200],
        )
    else:
        check("the refusal names the provenance and the fix", False, "no refusal")

    # ---- 2. an explicit, NAMED opt-out still gets the path -------------
    kind, res = _call(allow_stale=True, reason="unit test of the opt-out route")
    check("an explicitly-named allow_stale opt-out returns the path", kind == "ok", str(res))
    if kind == "ok":
        check(
            "the returned handle carries provenance env-stale",
            res.provenance == mod.ENV_STALE and not res.is_fresh,
            getattr(res, "provenance", "<no provenance attribute>"),
        )
        check("the handle is still usable as a path", str(res) == str(stale), str(res))
    else:
        check("the returned handle carries provenance env-stale", False, "refused")
        check("the handle is still usable as a path", False, "refused")

    # ---- 3. an UNNAMED opt-out is itself refused -----------------------
    kind, res = _call(allow_stale=True)
    check("allow_stale without a reason is refused", kind == "refused", f"{kind}: {res}")

    # ---- 4. a FRESH MINDC_SO is accepted -------------------------------
    fresh = tmp / "fresh.so"
    fresh.write_bytes(b"\x7fELF fresh")
    os.utime(fresh, None)  # now — newer than every stamped input
    os.environ["MINDC_SO"] = str(fresh)
    kind, res = _call()
    check("a MINDC_SO newer than every compile input is ACCEPTED", kind == "ok", str(res))
    if kind == "ok":
        check(
            "the accepted handle is marked fresh (env-verified)",
            res.provenance == mod.ENV_VERIFIED and res.is_fresh,
            getattr(res, "provenance", "<none>"),
        )
    else:
        check("the accepted handle is marked fresh (env-verified)", False, "refused")

    # ---- 5. a stamp sidecar rehabilitates an old mtime ------------------
    sidecar = tmp / "sidecar.so"
    sidecar.write_bytes(b"\x7fELF sidecar")
    (tmp / "sidecar.so.stamp").write_text(mod.stamp())
    os.utime(sidecar, (old, old))
    os.environ["MINDC_SO"] = str(sidecar)
    kind, res = _call()
    check(
        "an old mtime with a MATCHING stamp sidecar is accepted",
        kind == "ok" and getattr(res, "provenance", None) == mod.ENV_VERIFIED,
        f"{kind}: {res}",
    )
    (tmp / "sidecar.so.stamp").write_text("0" * 64)
    kind, res = _call()
    check("a NON-matching stamp sidecar does not rehabilitate it", kind == "refused", f"{kind}: {res}")

    # ---- 6. fail-closed on an ABSENT MINDC_SO is preserved -------------
    os.environ["MINDC_SO"] = str(tmp / "nope.so")
    kind, res = _call()
    check("a set-but-missing MINDC_SO still fails closed", kind == "refused", f"{kind}: {res}")
    kind, res = _call(allow_stale=True, reason="absence must fail closed even here")
    check(
        "allow_stale does NOT weaken the missing-file refusal",
        kind == "refused",
        f"{kind}: {res}",
    )

    # ---- 7. the legacy in-tree fallback is refused by default ----------
    _clear_env()
    os.environ["MINDC_SO_NOBUILD"] = "1"
    kind, res = _call()
    check("MINDC_SO_NOBUILD (legacy in-tree artifact) is REFUSED by default", kind == "refused", f"{kind}: {res}")
    kind, res = _call(allow_stale=True, reason="unit test of the legacy route")
    check(
        "the legacy route is tagged legacy-in-tree and sets USED_LEGACY_FALLBACK",
        kind == "ok"
        and getattr(res, "provenance", None) == mod.LEGACY_IN_TREE
        and mod.USED_LEGACY_FALLBACK,
        f"{kind}: {res}",
    )
    _clear_env()

    # ---- 8. the opt-out list is EXPLICIT and enumerable ----------------
    # F109's caller-side half: nothing linted the "callers must qualify" contract.
    # Strict-by-default makes refusal the inherited behaviour, so the only thing
    # left to police is the opt-out list — which must stay small and named.
    # Match a CALL that opts out, not a prose mention of the keyword — a comment
    # explaining the contract is not an opt-out, and a negative assertion that a
    # comment can trip is a gate that will be silenced by rewording.
    optout_re = re.compile(r"resolve_so\s*\([^)]*allow_stale\s*=\s*True", re.S)
    check(
        "the opt-out detector actually detects an opt-out (positive control)",
        bool(optout_re.search('so = resolve_so(allow_stale=True, reason="x")'))
        and not optout_re.search("# never call resolve_so with allow_stale=True"),
        "the detector must fire on a call and not on a comment",
    )
    optouts = []
    for py in sorted(_HERE.rglob("*.py")):
        if py.name == "_selfhost_so.py":
            continue
        if optout_re.search(py.read_text(encoding="utf-8", errors="replace")):
            optouts.append(py.relative_to(_HERE).as_posix())
    print(f"  note stale-tolerant opt-out CALL SITES in examples/mindc_mind: {optouts or 'none'}")
    check(
        "no smoke opts out of the freshness contract without this gate listing it",
        all(o in ALLOWED_OPTOUTS for o in optouts),
        f"unlisted opt-outs: {sorted(set(optouts) - ALLOWED_OPTOUTS)}",
    )

    # The sanctioned count-marker shape (scripts/gate_assert.py MARKER_RES); a
    # bare `ran=/fail=` is a number the gate typed rather than one it earned,
    # and examples/mindc_mind/smoke_wiring_lint.py refuses it for that reason.
    print(f"\nSDLC-GATE selfhost_so_provenance ran={RAN} fail={len(FAILED)}")
    if FAILED:
        print("FAIL: " + "; ".join(FAILED))
        return 1
    if RAN < 15:
        print(f"FAIL: only {RAN} assertions ran — a shrunken gate is not a green gate.")
        return 1
    print("PASS: resolve_so() qualifies its result and refuses a non-fresh oracle.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
