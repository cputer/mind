#!/usr/bin/env python3
"""Harness controls for `self_host_loop_smoke.py --advance`.

WHAT THIS FILE IS, AND IS NOT
-----------------------------
These are HARNESS controls. Every ELF here is a synthetic byte blob shaped to
satisfy `is_static_elf` (magic + ET_EXEC + length), and every oracle is a stub
object. Nothing here compiles anything, so **no case in this file is evidence
about the compiler, the frozen bootstrap, or the self-host loop closing.** What
they test is the harness's decision logic: which conditions publish the fixture,
which refuse it, and whether a refusal can leave the fixture damaged.

The one real boundary that IS exercised for real is the process boundary in
`case_advance_refused_when_the_old_seed_rejects_the_source`: it installs a
genuinely executable seed that exits non-zero, so `run_elf`'s subprocess path,
its non-zero handling and the caller's refusal all run unmocked. The loop's
other hops cannot be made real here — a payload must begin with `\\x7fELF` to
pass `is_static_elf` and therefore cannot also be a `#!` script — so those hops
stub `run_elf` and say so.

The contract under test
-----------------------
`--advance` seeds from the EXISTING frozen pure-MIND ELF, derives stage1 from the
CURRENT source, derives stage2/stage3 from stage1, and requires
`stage1 == stage2 == stage3`. Unlike ordinary verification the new fixed point MAY
differ from the old seed — that is the whole point of the mode. It additionally
requires a present, fresh Rust oracle emitting those same bytes. Only when every
check passes may `stage1.elf` and `MANIFEST.txt` be written.

Ordinary refusals assert the fixture is unchanged. A failed rollback instead
must report a torn publication and preserve recoverable copies; it cannot claim
the old files are intact.

Run:  python3 tests/self_host_loop_advance_test.py
Exit: 0 = every case passed, 1 = a case failed (the offending cases are named).
"""
from __future__ import annotations

import hashlib
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
SMOKE_DIR = REPO / "examples" / "mindc_mind"
PREFLIGHT = REPO / "scripts" / "preflight.sh"
sys.path.insert(0, str(SMOKE_DIR))

# Importing the module under test must not build anything or exit; that property
# is asserted for real in case_import_does_not_resolve_or_build_the_oracle, from
# a CLEAN subprocess where this import has not already happened.
#
# The guard below exists because that case cannot report a regression it is
# killed by: if import-time resolution comes back, THIS import dies first and the
# whole file exits before a single case runs. Measured with the eager
# `resolve_so()` restored: exit 1 with the resolver's refusal and not one `[FAIL]`
# line — correct-but-anonymous, and easy to misread as "no failures". Naming it
# costs four lines and keeps the non-zero exit.
try:
    import self_host_loop_smoke as loop  # noqa: E402
except SystemExit as exc:  # pragma: no cover - only on the regression
    print("[FAIL] import: importing self_host_loop_smoke must not exit "
          f"(the oracle is being resolved at import time): {exc}")
    raise

FAILURES: list[str] = []


def describe(value: object) -> str:
    if isinstance(value, bytes):
        return f"bytes(len={len(value)}, sha256={hashlib.sha256(value).hexdigest()})"
    if isinstance(value, (list, tuple)):
        return "[" + ", ".join(describe(item) for item in value) + "]"
    text = repr(value)
    return text if len(text) <= 240 else text[:237] + "..."


def check(label: str, got: object, want: object) -> None:
    ok = got == want
    print(f"[{'PASS' if ok else 'FAIL'}] {label}: got={describe(got)} want={describe(want)}")
    if not ok:
        FAILURES.append(label)


# ── synthetic material ─────────────────────────────────────────────────────

def elf(tag: bytes, size: int = 5000) -> bytes:
    """A byte blob that satisfies `is_static_elf` and is unique per `tag`.

    Shaped, not compiled: magic at 0, ET_EXEC at 16, padded past the 4096-byte
    floor. It is a stand-in for an emitted image so the harness's comparisons
    have something to compare; it is not an executable and is not compiler
    output.
    """
    head = b"\x7fELF" + b"\x00" * 12 + b"\x02\x00" + tag
    return head + b"\x00" * (size - len(head))


def sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


OLD_SEED = elf(b"old-seed")
NEW_POINT = elf(b"new-fixed-point")
OTHER = elf(b"something-else")
SRC = b"// synthetic combined source\n"
IMAGE = b"\x00" * 16 + SRC
USER_LO = 7


class StubOracle:
    """Stands in for `ResolvedSo` at the ctypes boundary.

    Carries the same three things `do_advance` consults — existence, freshness,
    provenance — so a stale or absent oracle can be presented without building
    one.
    """

    def __init__(self, *, present: bool = True, provenance: str = "fresh-build") -> None:
        self._present = present
        self.provenance = provenance
        self.detail = f"stub oracle ({provenance})"
        self.name = "stub-oracle.so"

    def exists(self) -> bool:
        return self._present

    @property
    def is_fresh(self) -> bool:
        return self.provenance in ("fresh-build", "env-verified")

    def __str__(self) -> str:  # used in the refusal messages
        return "<stub oracle>"


class Sandbox:
    """A throwaway fixture pair with the module's paths pointed at it.

    Restores every patched module attribute on exit so one case cannot leak
    into the next, and never touches the real tracked fixture.
    """

    def __init__(self) -> None:
        self.dir = pathlib.Path(tempfile.mkdtemp(prefix="advance-control-"))
        self.elf_path = self.dir / "stage1.elf"
        self.manifest_path = self.dir / "MANIFEST.txt"
        self.elf_path.write_bytes(OLD_SEED)
        self.elf_path.chmod(0o755)
        self.manifest_path.write_text("# original manifest\nstage1.elf\t1\tdeadbeef\n")
        self.before_elf = self.elf_path.read_bytes()
        self.before_manifest = self.manifest_path.read_bytes()
        self._saved: dict[str, object] = {}

    def patch(self, **kw: object) -> None:
        for k, v in kw.items():
            self._saved.setdefault(k, getattr(loop, k))
            setattr(loop, k, v)

    def __enter__(self) -> "Sandbox":
        self.patch(_FROZEN=self.elf_path, _FROZEN_MANIFEST=self.manifest_path)
        return self

    def __exit__(self, *exc: object) -> None:
        for k, v in self._saved.items():
            setattr(loop, k, v)
        shutil.rmtree(self.dir, ignore_errors=True)

    def unchanged(self) -> bool:
        """Both published files still byte-identical to their pre-run state."""
        return (
            self.elf_path.read_bytes() == self.before_elf
            and self.manifest_path.read_bytes() == self.before_manifest
        )


def stub_loop(stages, log: list[bytes], *, on_call=None, before_call=None):
    """Replace `run_elf` with a recorder that returns the given stage chain.

    Records the BYTES OF THE FILE it was asked to run, not its path. The harness
    now executes a private snapshot of the captured old seed rather than the live
    fixture, so "the old seed ran first" has to be asserted by content — which is
    the stronger claim anyway, and the one the recorded
    `advanced_from_seed_sha256` is supposed to describe.

    An entry in `stages` may be an exception instance, which is raised instead of
    returned; that is how a stage timeout or spawn error is presented.

    `before_call` fires BEFORE the file is read, `on_call` after — the two sides
    of the time-of-check/time-of-use window a case may need to open.
    """
    seq = list(stages)

    def _run(path: pathlib.Path, stdin_image: bytes) -> bytes:
        if before_call is not None:
            before_call(len(log) + 1)
        log.append(pathlib.Path(path).read_bytes())
        if on_call is not None:
            on_call(len(log))
        item = seq[min(len(log) - 1, len(seq) - 1)]
        if isinstance(item, BaseException):
            raise item
        return item

    return _run


# ── the positive control ───────────────────────────────────────────────────

def case_advance_publishes_a_new_fixed_point() -> None:
    """A fixed point DISTINCT from the old seed is published, seeded by the old
    seed, and the manifest carries the full receipt."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda combined, user_lo: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)

        check("advance: publishes on full agreement", rc, 0)
        check("advance: the new point differs from the old seed",
              NEW_POINT != OLD_SEED, True)
        check("advance: stage1.elf now holds the new fixed point",
              sb.elf_path.read_bytes(), NEW_POINT)
        check("advance: published ELF is executable",
              bool(sb.elf_path.stat().st_mode & 0o111), True)

        # The old seed must be the FIRST thing invoked, and it is asserted by
        # CONTENT: the harness executes a private snapshot, so a path check would
        # pass on any file that happened to sit there. This is what makes the
        # recorded advanced_from_seed_sha256 describe the thing that actually ran.
        check("advance: the bytes that ran first are the captured old seed",
              log[:1], [OLD_SEED])
        check("advance: the snapshot was not the live fixture path",
              sb.elf_path.read_bytes() == NEW_POINT, True)
        check("advance: three stages were derived", len(log), 3)

        text = sb.manifest_path.read_text()
        for key, value in (
            ("advanced_from_seed_sha256", sha(OLD_SEED)),
            ("combined_source_sha256", sha(SRC)),
            ("stage1_sha256", sha(NEW_POINT)),
            ("stage2_sha256", sha(NEW_POINT)),
            ("stage3_sha256", sha(NEW_POINT)),
            ("oracle_sha256", sha(NEW_POINT)),
        ):
            check(f"advance receipt records {key}", f"# {key}\t{value}" in text, True)
        check("advance receipt names the publishing mode",
              "published-by\tself_host_loop_smoke.py --advance" in text, True)
        check("advance receipt records the row for the new ELF",
              f"stage1.elf\t{len(NEW_POINT)}\t{sha(NEW_POINT)}" in text, True)


# ── refusals: every one must leave BOTH files untouched ────────────────────

def case_advance_refused_when_the_old_seed_rejects_the_source() -> None:
    """REAL process boundary: an executable seed that exits non-zero.

    `run_elf` is NOT stubbed here — a real subprocess is spawned, exits 3, and
    the refusal travels through `run_elf`'s non-zero handling into `do_advance`.
    """
    with Sandbox() as sb:
        sb.elf_path.write_text("#!/bin/sh\necho 'refused' >&2\nexit 3\n")
        sb.elf_path.chmod(0o755)
        sb.before_elf = sb.elf_path.read_bytes()
        sb.patch(so=lambda: StubOracle(), stage0_emit=lambda c, u: NEW_POINT)
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: old-seed refusal fails", rc, 1)
        check("advance: old-seed refusal leaves the fixture untouched",
              sb.unchanged(), True)


def case_advance_refused_on_stage_mismatch() -> None:
    """stage1 != stage2 is not a fixed point; a non-reproducing compiler must
    never become the seed."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, OTHER, OTHER), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: stage mismatch fails", rc, 1)
        check("advance: stage mismatch leaves the fixture untouched",
              sb.unchanged(), True)


def case_advance_refused_on_non_elf_stage() -> None:
    """A later stage that is not a static ELF is refused (the first stage is
    already screened inside run_loop_from_seed)."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, b"not an elf", b"not an elf"), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: non-ELF stage fails", rc, 1)
        check("advance: non-ELF stage leaves the fixture untouched",
              sb.unchanged(), True)


def case_advance_refused_when_the_oracle_is_missing() -> None:
    """One witness is not corroboration: with no oracle the advance is blocked."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(present=False),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: missing oracle is blocked", rc, 2)
        check("advance: missing oracle leaves the fixture untouched",
              sb.unchanged(), True)


def case_real_resolver_refuses_an_explicitly_missing_oracle_before_load() -> None:
    """The real resolver must reject a missing MINDC_SO before ctypes is used.

    The normal advance cases use a stub oracle to stay compiler-free. This case
    deliberately uses the production `so()`/`resolve_so()` path and only stubs
    the old-seed loop, so a missing explicit handle cannot be mistaken for a
    synthetic oracle result.
    """
    with Sandbox() as sb:
        log: list[bytes] = []
        loaded: list[object] = []
        prior_so = loop._SO
        resolver_globals = loop.resolve_so.__globals__
        prior_provenance = resolver_globals["PROVENANCE"]
        prior_legacy_fallback = resolver_globals["USED_LEGACY_FALLBACK"]
        prior_env = os.environ.get("MINDC_SO")
        real_cdll = loop.ctypes.CDLL
        missing = sb.dir / "missing-oracle.so"

        def forbidden_load(*args, **kwargs):
            loaded.append(args[0] if args else None)
            raise AssertionError("ctypes must not load a resolver-refused oracle")

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            _SO=None,
        )
        loop.ctypes.CDLL = forbidden_load
        os.environ["MINDC_SO"] = str(missing)
        caught: BaseException | None = None
        try:
            loop.do_advance(SRC, IMAGE, USER_LO)
        except BaseException as exc:  # strict resolver uses SystemExit
            caught = exc
        finally:
            loop.ctypes.CDLL = real_cdll
            loop._SO = prior_so
            resolver_globals["PROVENANCE"] = prior_provenance
            resolver_globals["USED_LEGACY_FALLBACK"] = prior_legacy_fallback
            if prior_env is None:
                os.environ.pop("MINDC_SO", None)
            else:
                os.environ["MINDC_SO"] = prior_env

        check("real resolver: missing MINDC_SO is refused", isinstance(caught, SystemExit), True)
        check("real resolver: missing path is named",
              "MINDC_SO" in str(caught), True)
        check("real resolver: ctypes was never loaded", loaded, [])
        check("real resolver: missing-oracle fixture is unchanged", sb.unchanged(), True)


def case_real_resolver_refuses_a_deliberately_old_oracle_before_load() -> None:
    """A real explicit artifact older than the compile inputs is refused."""
    with Sandbox() as sb:
        log: list[bytes] = []
        loaded: list[object] = []
        prior_so = loop._SO
        resolver_globals = loop.resolve_so.__globals__
        prior_provenance = resolver_globals["PROVENANCE"]
        prior_legacy_fallback = resolver_globals["USED_LEGACY_FALLBACK"]
        prior_env = os.environ.get("MINDC_SO")
        real_cdll = loop.ctypes.CDLL
        old_oracle = sb.dir / "old-oracle.so"
        old_oracle.write_bytes(b"deliberately old oracle")
        os.utime(old_oracle, ns=(1, 1))

        def forbidden_load(*args, **kwargs):
            loaded.append(args[0] if args else None)
            raise AssertionError("ctypes must not load a stale resolver oracle")

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            _SO=None,
        )
        loop.ctypes.CDLL = forbidden_load
        os.environ["MINDC_SO"] = str(old_oracle)
        caught: BaseException | None = None
        try:
            loop.do_advance(SRC, IMAGE, USER_LO)
        except BaseException as exc:  # strict resolver uses SystemExit
            caught = exc
        finally:
            loop.ctypes.CDLL = real_cdll
            loop._SO = prior_so
            resolver_globals["PROVENANCE"] = prior_provenance
            resolver_globals["USED_LEGACY_FALLBACK"] = prior_legacy_fallback
            if prior_env is None:
                os.environ.pop("MINDC_SO", None)
            else:
                os.environ["MINDC_SO"] = prior_env

        check("real resolver: old MINDC_SO is refused", isinstance(caught, SystemExit), True)
        check("real resolver: stale provenance is named",
              "NON-FRESH" in str(caught), True)
        check("real resolver: stale oracle was never loaded", loaded, [])
        check("real resolver: stale-oracle fixture is unchanged", sb.unchanged(), True)


def case_advance_refused_on_a_stale_oracle() -> None:
    """Bytes of unknown age cannot corroborate a fixed point derived from the
    CURRENT source, even when they would agree."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(provenance="legacy-in-tree"),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: stale oracle is blocked", rc, 2)
        check("advance: stale oracle leaves the fixture untouched",
              sb.unchanged(), True)


def case_advance_refused_when_the_oracle_disagrees() -> None:
    """Two independent compilers disagreeing about the current source means
    neither may be frozen."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: OTHER,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: oracle mismatch fails", rc, 1)
        check("advance: oracle mismatch leaves the fixture untouched",
              sb.unchanged(), True)


def case_advance_oracle_leg_is_not_deferrable() -> None:
    """The deferral env var records a one-leg VERIFICATION run. It must not buy
    a publication with the corroborating witness switched off."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(present=False),
        )
        prior = os.environ.get(loop.ORACLE_DEFER_ENV)
        os.environ[loop.ORACLE_DEFER_ENV] = "1"
        try:
            rc = loop.do_advance(SRC, IMAGE, USER_LO)
        finally:
            if prior is None:
                os.environ.pop(loop.ORACLE_DEFER_ENV, None)
            else:
                os.environ[loop.ORACLE_DEFER_ENV] = prior
        check("advance: oracle deferral does not buy a publication", rc, 2)
        check("advance: deferred-oracle attempt leaves the fixture untouched",
              sb.unchanged(), True)


def case_advance_rolls_back_when_publication_fails_part_way() -> None:
    """The ELF is replaced, the manifest replacement then fails, and the ELF is
    rolled back to exactly its previous bytes.

    Two independent paths cannot be swapped atomically, so this is the case that
    decides whether a torn publication is repaired or left on disk.
    """
    with Sandbox() as sb:
        log: list[bytes] = []
        calls: list[int] = []
        real_replace = os.replace

        def flaky(src, dst):
            calls.append(1)
            if len(calls) == 2:  # the manifest replacement
                raise OSError(28, "No space left on device (planted)")
            return real_replace(src, dst)

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
            _replace=flaky,
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: publication failure fails the run", rc, 1)
        check("advance: the ELF replacement was actually attempted",
              len(calls) >= 2, True)
        check("advance: publication failure rolls the ELF back",
              sb.elf_path.read_bytes(), sb.before_elf)
        check("advance: publication failure leaves the manifest untouched",
              sb.manifest_path.read_bytes(), sb.before_manifest)
        check("advance: no staging file is left behind",
              sorted(p.name for p in sb.dir.iterdir()),
              ["MANIFEST.txt", "stage1.elf"])


def case_advance_blocked_when_there_is_no_seed_to_advance_from() -> None:
    """--advance is defined by the seed it starts from; without one it is the
    --reseed case and must say so rather than mint anything."""
    with Sandbox() as sb:
        sb.elf_path.unlink()
        tripped: list[str] = []
        sb.patch(
            run_elf=lambda *a, **k: tripped.append("run_elf") or b"",
            so=lambda: tripped.append("so") or StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("advance: missing seed is blocked", rc, 2)
        check("advance: missing seed runs nothing", tripped, [])
        check("advance: missing seed does not create an ELF",
              sb.elf_path.exists(), False)
        check("advance: missing seed leaves the manifest untouched",
              sb.manifest_path.read_bytes(), sb.before_manifest)


# ── mode selection ─────────────────────────────────────────────────────────

def case_advance_and_reseed_together_are_refused_before_any_work() -> None:
    """All four request shapes are refused, and the refusal happens before the
    source is read, before any process runs and before any oracle is resolved."""
    shapes = (
        ("flag+flag", ["--advance", "--reseed"], {}),
        ("flag+env", ["--advance"], {loop.RESEED_ENV: "1"}),
        ("env+flag", ["--reseed"], {loop.ADVANCE_ENV: "1"}),
        ("env+env", [], {loop.ADVANCE_ENV: "1", loop.RESEED_ENV: "1"}),
    )
    for label, argv, env in shapes:
        with Sandbox() as sb:
            tripped: list[str] = []
            sb.patch(
                build_seed=lambda: tripped.append("build_seed") or (SRC, IMAGE, USER_LO),
                run_elf=lambda *a, **k: tripped.append("run_elf") or b"",
                stage0_emit=lambda *a, **k: tripped.append("stage0_emit") or b"",
                so=lambda: tripped.append("so") or StubOracle(),
            )
            rc = loop.main(argv, env)
            check(f"conflict({label}): blocked", rc, 2)
            check(f"conflict({label}): nothing read, run or resolved", tripped, [])
            check(f"conflict({label}): fixture untouched", sb.unchanged(), True)


def case_a_single_mode_request_is_not_a_conflict() -> None:
    """The conflict gate must not refuse the ordinary single-mode requests it
    exists to disambiguate — otherwise it would be a mode killer, not a gate."""
    cases = (
        ("advance flag", ["--advance"], {}, ["--advance"], []),
        ("reseed flag", ["--reseed"], {}, [], ["--reseed"]),
        ("advance env", [], {loop.ADVANCE_ENV: "1"}, [f"{loop.ADVANCE_ENV}=1"], []),
        ("reseed env", [], {loop.RESEED_ENV: "1"}, [], [f"{loop.RESEED_ENV}=1"]),
        ("neither", [], {}, [], []),
    )
    for label, argv, env, want_adv, want_res in cases:
        adv, res = loop.requested_modes(argv, env)
        check(f"modes({label}): advance sources", adv, want_adv)
        check(f"modes({label}): reseed sources", res, want_res)
        check(f"modes({label}): no conflict", loop.mode_conflict(adv, res), None)


# ── the ordinary mode must not have been weakened ──────────────────────────

def case_verify_mode_still_requires_the_frozen_fixed_point() -> None:
    """Ordinary verification keeps asserting stage1 == the frozen seed. --advance
    is the ONLY mode where a new fixed point is acceptable."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            build_seed=lambda: (SRC, IMAGE, USER_LO),
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.main([], {})
        check("verify: a new fixed point is still a failure", rc, 1)
        check("verify: failure leaves the fixture untouched", sb.unchanged(), True)


def case_verify_mode_still_requires_a_fresh_oracle() -> None:
    """A stale oracle that agrees with an equally stale seed still fails."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            build_seed=lambda: (SRC, IMAGE, USER_LO),
            run_elf=stub_loop((OLD_SEED, OLD_SEED, OLD_SEED), log),
            stage0_emit=lambda c, u: OLD_SEED,
            so=lambda: StubOracle(provenance="legacy-in-tree"),
        )
        rc = loop.main([], {})
        check("verify: a stale agreeing oracle still fails", rc, 1)
        check("verify: stale-oracle failure leaves the fixture untouched",
              sb.unchanged(), True)


def case_verify_mode_passes_on_an_unchanged_fixed_point() -> None:
    """Positive control for the ordinary mode: the unmodified path still passes,
    so the failures above are the assertions firing and not the mode broken."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            build_seed=lambda: (SRC, IMAGE, USER_LO),
            run_elf=stub_loop((OLD_SEED, OLD_SEED, OLD_SEED), log),
            stage0_emit=lambda c, u: OLD_SEED,
            so=lambda: StubOracle(),
        )
        rc = loop.main([], {})
        check("verify: unchanged fixed point + fresh oracle passes", rc, 0)
        check("verify: a passing run publishes nothing", sb.unchanged(), True)


# ── the receipt ────────────────────────────────────────────────────────────

def case_receipt_is_reproducible_and_carries_no_timestamp() -> None:
    """Same inputs, same bytes — the receipt is derived, never observed.

    A timestamp cannot be re-derived, so it could not be checked by anyone
    later; it would also make the file differ from itself on every run.
    """
    receipt = {"advanced_from_seed_sha256": sha(OLD_SEED), "stage1_sha256": sha(NEW_POINT)}
    first = loop.manifest_text(NEW_POINT, published_by="x --advance", receipt=receipt)
    second = loop.manifest_text(NEW_POINT, published_by="x --advance", receipt=receipt)
    check("receipt: identical inputs give identical bytes", first, second)
    check("receipt: records the ELF it belongs to",
          f"stage1.elf\t{len(NEW_POINT)}\t{sha(NEW_POINT)}" in first, True)
    # A torn pair is detectable precisely because the manifest names the ELF's
    # hash; assert that hash is the published one and not a placeholder.
    check("receipt: the recorded hash is the new point's",
          sha(NEW_POINT) in first, True)
    # Scan the RECEIPT ROWS only (`# <key>\t<value>`), not the prose header —
    # the header explains why there is no timestamp, and a naive substring
    # search over the whole file matches that explanation instead of a field.
    rows = [ln[2:].split("\t", 1)[0] for ln in first.splitlines()
            if ln.startswith("# ") and "\t" in ln]
    check("receipt: rows were actually found (the scan is not vacuous)",
          "stage1_sha256" in rows, True)
    clockish = [k for k in rows
                if any(w in k.lower() for w in ("time", "date", "unix", "clock"))]
    check("receipt: no wall-clock field is emitted", clockish, [])


def case_import_does_not_resolve_or_build_the_oracle() -> None:
    """Importing the harness must be a pure operation.

    `resolve_so()` may shell out to `mindc build --emit=cdylib` and exits the
    process on a non-fresh oracle. Doing that at import time made a plain
    `import self_host_loop_smoke` — which a sibling gate and this test both do —
    able to launch a compiler build nobody asked for. Run in a CLEAN subprocess
    so this file's own import cannot mask the regression.
    """
    probe = (
        "import sys; sys.path.insert(0, %r)\n"
        "import self_host_loop_smoke as m\n"
        "print('SO=', m._SO)\n" % str(SMOKE_DIR)
    )
    r = subprocess.run(
        [sys.executable, "-c", probe],
        capture_output=True, text=True, timeout=120, cwd=str(REPO),
    )
    check("import: exits cleanly", r.returncode, 0)
    check("import: oracle is not resolved", "SO= None" in r.stdout, True)
    check("import: no build was attempted",
          "mindc build" in (r.stdout + r.stderr), False)


# ── review round 2: staging, snapshot identity, rollback, attribution ──────

def case_staging_ignores_an_occupied_or_symlinked_conventional_name() -> None:
    """The old fixed `<name>.advance-staging` path is inert.

    It was two bugs: a symlink there would have been followed by an ordinary
    `open(..., "wb")`, redirecting the staged bytes anywhere the link pointed;
    and two concurrent runs shared the one name. Exclusive unique siblings fix
    both. Planted here as BOTH a symlink and a regular file, so publication has
    to ignore the name entirely rather than merely handle one shape.
    """
    with Sandbox() as sb:
        canary = sb.dir / "canary-outside-the-fixture"
        canary.write_bytes(b"canary must not be written through a staging symlink")
        before_canary = canary.read_bytes()
        link = sb.dir / "stage1.elf.advance-staging"
        os.symlink(canary, link)
        squatter = sb.dir / "MANIFEST.txt.advance-staging"
        squatter.write_bytes(b"an occupant at the old conventional name")
        before_squatter = squatter.read_bytes()

        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)

        check("staging: publication still succeeds", rc, 0)
        check("staging: the symlink target was not written",
              canary.read_bytes(), before_canary)
        check("staging: the symlink itself is untouched",
              link.is_symlink() and os.readlink(link) == str(canary), True)
        check("staging: the occupied conventional name is untouched",
              squatter.read_bytes(), before_squatter)
        check("staging: the fixture really was published",
              sb.elf_path.read_bytes(), NEW_POINT)
        check("staging: no staging debris remains",
              [q.name for q in sb.dir.iterdir() if q.name.startswith(".advance-stage-")],
              [])


def case_publication_preserves_the_existing_file_modes() -> None:
    """Publishing must not silently re-permission the fixture."""
    with Sandbox() as sb:
        sb.elf_path.chmod(0o750)
        sb.manifest_path.chmod(0o640)
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("modes: published", rc, 0)
        check("modes: ELF mode preserved", sb.elf_path.stat().st_mode & 0o7777, 0o750)
        check("modes: manifest mode preserved",
              sb.manifest_path.stat().st_mode & 0o7777, 0o640)


def case_the_executed_seed_is_a_snapshot_not_the_live_path() -> None:
    """The bytes that were HASHED are the bytes that RAN.

    Hashing `_FROZEN` and then executing `_FROZEN` again is a
    time-of-check/time-of-use gap: a write landing in between makes the recorded
    `advanced_from_seed_sha256` describe a file that never ran. This case opens
    that window deliberately — the live fixture is replaced immediately before
    the first execution — so a harness that runs a private snapshot still
    executes the captured bytes, while one that re-opens the live path executes
    the impostor. The hook is on `run_elf`, which BOTH shapes call, so the case
    cannot be dodged by simply not creating a snapshot.
    """
    with Sandbox() as sb:
        log: list[bytes] = []

        def swap_the_live_fixture(n: int) -> None:
            if n == 1:
                sb.elf_path.write_bytes(OTHER)

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log,
                              before_call=swap_the_live_fixture),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("snapshot: the executed bytes are the CAPTURED old seed",
              log[:1], [OLD_SEED])
        check("snapshot: the impostor was never executed", OTHER in log, False)
        # And because the live fixture did change, publication is refused too.
        check("snapshot: publication refused over the changed fixture", rc, 1)
        check("snapshot: the changed fixture was not overwritten",
              sb.elf_path.read_bytes(), OTHER)


def case_advance_refuses_to_overwrite_a_fixture_that_changed_mid_run() -> None:
    """A concurrent advance that completed while this one ran must not be
    silently discarded."""
    with Sandbox() as sb:
        log: list[bytes] = []

        def meddle(n: int) -> None:
            if n == 1:  # someone else publishes while we are deriving stages
                sb.elf_path.write_bytes(OTHER)

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log, on_call=meddle),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("changed seed: refused", rc, 1)
        check("changed seed: the other advance was not overwritten",
              sb.elf_path.read_bytes(), OTHER)
        check("changed seed: the manifest was not published",
              sb.manifest_path.read_bytes(), sb.before_manifest)
        check("changed seed: the snapshot still ran the CAPTURED bytes",
              log[:1], [OLD_SEED])


def case_rollback_needs_no_new_allocation_after_the_failure() -> None:
    """The rollback copy is written BEFORE either live file is touched.

    If it were written only after the manifest replace failed, a full disk could
    fail the publish AND the rollback, stranding the new ELF beside the old
    manifest. Asserted by counting staging writes: none may happen after the
    failure.
    """
    with Sandbox() as sb:
        log: list[bytes] = []
        replaces: list[int] = []
        stages_at_failure: list[int] = []
        stage_calls: list[int] = []
        real_replace = os.replace
        real_stage = loop._stage

        def counting_stage(parent, data, *, mode):
            stage_calls.append(1)
            return real_stage(parent, data, mode=mode)

        def flaky(src, dst):
            replaces.append(1)
            if len(replaces) == 2:
                stages_at_failure.append(len(stage_calls))
                raise OSError(28, "No space left on device (planted)")
            return real_replace(src, dst)

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
            _stage=counting_stage,
            _replace=flaky,
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("rollback: publication failure fails the run", rc, 1)
        check("rollback: three files were staged up front", stages_at_failure, [3])
        check("rollback: no staging write happened after the failure",
              len(stage_calls), 3)
        check("rollback: the ELF is back to its previous bytes",
              sb.elf_path.read_bytes(), sb.before_elf)
        check("rollback: the manifest is untouched",
              sb.manifest_path.read_bytes(), sb.before_manifest)
        check("rollback: no staging debris remains",
              [q.name for q in sb.dir.iterdir() if q.name.startswith(".advance-stage-")],
              [])


def case_a_failed_rollback_is_reported_as_torn_with_backups_kept() -> None:
    """When the rollback ITSELF fails, the harness must not claim the old files
    are unchanged — it must say so and name the surviving copies."""
    with Sandbox() as sb:
        log: list[bytes] = []
        replaces: list[int] = []
        real_replace = os.replace

        def flaky(src, dst):
            replaces.append(1)
            if len(replaces) >= 2:  # the manifest publish AND the rollback
                raise OSError(28, "No space left on device (planted)")
            return real_replace(src, dst)

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
            _replace=flaky,
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("torn: the run fails", rc, 1)
        check("torn: the rollback was attempted", len(replaces) >= 3, True)
        # The honest state: the ELF really is the new one and the manifest is old.
        check("torn: the ELF is the unpublished-new one",
              sb.elf_path.read_bytes(), NEW_POINT)
        check("torn: the manifest is still the old one",
              sb.manifest_path.read_bytes(), sb.before_manifest)
        survivors = [q for q in sb.dir.iterdir() if q.name.startswith(".advance-stage-")]
        check("torn: recoverable copies were preserved", len(survivors) >= 1, True)
        check("torn: the old ELF is recoverable from one of them",
              any(q.read_bytes() == sb.before_elf for q in survivors), True)


def case_staging_failure_leaves_no_debris_and_touches_nothing() -> None:
    """A staging error is discovered while both live files are still intact."""
    with Sandbox() as sb:
        log: list[bytes] = []
        calls: list[int] = []
        real_stage = loop._stage

        def failing_stage(parent, data, *, mode):
            calls.append(1)
            if len(calls) == 2:  # the manifest's staging file
                raise OSError(28, "No space left on device (planted)")
            return real_stage(parent, data, mode=mode)

        sb.patch(
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
            _stage=failing_stage,
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("staging failure: the run fails", rc, 1)
        check("staging failure: fixture untouched", sb.unchanged(), True)
        check("staging failure: no debris remains",
              [q.name for q in sb.dir.iterdir() if q.name.startswith(".advance-stage-")],
              [])


def case_a_later_stage_failure_is_not_blamed_on_the_old_seed() -> None:
    """stage2/stage3 failures must not be reported as "the old seed refused the
    source" — by then the old seed has already compiled it successfully."""
    import io
    import contextlib

    for stage, chain in (
        (2, (NEW_POINT, b"not an elf", NEW_POINT)),
        (3, (NEW_POINT, NEW_POINT, b"not an elf")),
    ):
        with Sandbox() as sb:
            log: list[bytes] = []
            sb.patch(
                run_elf=stub_loop(chain, log),
                stage0_emit=lambda c, u: NEW_POINT,
                so=lambda: StubOracle(),
            )
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = loop.do_advance(SRC, IMAGE, USER_LO)
            out = buf.getvalue()
            check(f"attribution(stage{stage}): fails", rc, 1)
            check(f"attribution(stage{stage}): names the stage",
                  f"stage{stage}" in out, True)
            check(f"attribution(stage{stage}): does NOT blame the old seed",
                  "old frozen seed refused" in out, False)
            check(f"attribution(stage{stage}): fixture untouched", sb.unchanged(), True)


def case_a_non_elf_stage2_is_refused_before_it_is_executed() -> None:
    """The screen happens before the bytes are written out and run: a garbage
    stage2 must never reach execve as stage3's seed."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            run_elf=stub_loop((NEW_POINT, b"not an elf", NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.do_advance(SRC, IMAGE, USER_LO)
        check("non-ELF stage2: fails", rc, 1)
        check("non-ELF stage2: stage3 was never executed", len(log), 2)
        check("non-ELF stage2: fixture untouched", sb.unchanged(), True)


def case_stage_spawn_and_timeout_failures_are_structured_and_harmless() -> None:
    """A spawn error and a timeout both become a numbered stage failure, a
    non-zero verdict, and an untouched fixture."""
    for label, boom in (
        ("timeout", subprocess.TimeoutExpired(cmd="stage2.elf", timeout=120)),
        ("spawn", OSError(8, "Exec format error (planted)")),
    ):
        with Sandbox() as sb:
            log: list[bytes] = []
            sb.patch(
                run_elf=stub_loop((NEW_POINT, boom, NEW_POINT), log),
                stage0_emit=lambda c, u: NEW_POINT,
                so=lambda: StubOracle(),
            )
            rc = loop.do_advance(SRC, IMAGE, USER_LO)
            check(f"stage {label}: structured non-zero verdict", rc, 1)
            check(f"stage {label}: fixture untouched", sb.unchanged(), True)


def case_reseed_later_stage_failures_are_structured_and_harmless() -> None:
    """Legacy --reseed must identify stage2/3 failures and publish nothing.

    These controls use a stub fresh oracle and a synthetic stage1, but exercise
    the reseed path's real `_derive` calls. The malformed output, spawn error,
    and timeout cases all represent failures after the Rust seed emitted stage1.
    """
    import contextlib
    import io

    cases = (
        ("stage2-non-ELF", 2, (b"not an elf",)),
        ("stage2-spawn", 2,
         (OSError(8, "Exec format error (planted)"),)),
        ("stage2-timeout", 2,
         (subprocess.TimeoutExpired(cmd="stage1.elf", timeout=120),)),
        ("stage3-non-ELF", 3, (NEW_POINT, b"not an elf")),
    )
    for label, stage, chain in cases:
        with Sandbox() as sb:
            log: list[bytes] = []
            sb.patch(
                run_elf=stub_loop(chain, log),
                stage0_emit=lambda c, u: NEW_POINT,
                so=lambda: StubOracle(),
            )
            output = io.StringIO()
            raised: BaseException | None = None
            with contextlib.redirect_stdout(output):
                try:
                    rc = loop.do_reseed(SRC, IMAGE, USER_LO)
                except BaseException as exc:  # regression: raw OSError/traceback
                    raised = exc
                    rc = None
            text = output.getvalue()
            check(f"reseed({label}): returns a failure", rc, 1)
            check(f"reseed({label}): does not raise", raised, None)
            check(f"reseed({label}): names stage{stage}", f"stage{stage}:" in text, True)
            check(f"reseed({label}): fixture untouched", sb.unchanged(), True)


def case_preflight_self_host_branch_fails_closed_for_exit_statuses() -> None:
    """The actual preflight loop branch must aggregate BLOCKED as a failure.

    Extracting only that committed branch keeps this control independent of
    Cargo and compiler artifacts. A synthetic `python3` executable supplies
    each gate exit status. The real status dispatch calls a minimal aggregate
    `bad()` control; its log is redirected into this test's private directory.
    """
    source = PREFLIGHT.read_text(encoding="utf-8")
    start_marker = "  loop_rc=0; python3 scripts/run_gate.py --min-asserted 2 "
    end_marker = "\n  fi\n\n  step \"bench gate"
    start = source.index(start_marker)
    end = source.index(end_marker, start) + len("\n  fi")
    branch = source[start:end]

    for fake_rc, want_fail in ((0, 0), (1, 1), (2, 1)):
        with tempfile.TemporaryDirectory(prefix="preflight-control-") as td:
            root = pathlib.Path(td)
            fake_bin = root / "bin"
            fake_bin.mkdir()
            fake_python = fake_bin / "python3"
            fake_python.write_text(
                "#!/bin/sh\n"
                "echo \"synthetic runner rc=$FAKE_RC\"\n"
                "if [ \"$FAKE_RC\" = 2 ]; then echo 'BLOCKED synthetic'; fi\n"
                "if [ \"$FAKE_RC\" = 1 ]; then echo 'FAIL synthetic'; fi\n"
                "exit \"$FAKE_RC\"\n",
                encoding="utf-8",
            )
            fake_python.chmod(0o755)
            script = root / "preflight-loop-snippet.sh"
            script.write_text(
                "#!/bin/sh\n"
                "fail=0\n"
                "bad() { fail=1; }\n"
                + branch.replace("/tmp/preflight-loop.out", str(root / "loop.out"))
                + "\nprintf 'aggregate_fail=%s\\n' \"$fail\"\n",
                encoding="utf-8",
            )
            script.chmod(0o755)
            env = dict(os.environ)
            env.update({"PATH": f"{fake_bin}{os.pathsep}{env['PATH']}",
                        "FAKE_RC": str(fake_rc)})
            result = subprocess.run(
                ["bash", str(script)], cwd=str(REPO), env=env,
                capture_output=True, text=True, timeout=30,
            )
            check(f"preflight synthetic exit{fake_rc}: snippet runs",
                  result.returncode, 0)
            check(f"preflight synthetic exit{fake_rc}: aggregate bad count",
                  f"aggregate_fail={want_fail}" in result.stdout, True)


def case_derive_labels_the_stage_it_failed_on() -> None:
    """`LoopFailure` carries the stage number, which is what the attribution
    above is built on. Asserted directly so a silent renumbering is caught."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(run_elf=stub_loop((NEW_POINT, b"garbage", NEW_POINT), log))
        seed = sb.dir / "seed.elf"
        seed.write_bytes(OLD_SEED)
        seed.chmod(0o755)
        try:
            loop.run_loop_from_seed(seed, IMAGE, sb.dir)
            failed = None
        except loop.LoopFailure as e:
            failed = e.stage
        check("derive: the failing stage is identified", failed, 2)
        check("derive: LoopFailure is a RuntimeError (verify path still catches it)",
              issubclass(loop.LoopFailure, RuntimeError), True)


# ── dispatch through the REAL main(), not do_advance directly ──────────────

def case_main_advance_does_not_resolve_the_oracle_before_admission() -> None:
    """Order through the real entry point: mode gate, then seed admission, and
    only then the oracle.

    Resolving early is not free — `so()` can shell out to `mindc build
    --emit=cdylib` — so an --advance with no seed to advance from must refuse
    without ever building one.
    """
    with Sandbox() as sb:
        sb.elf_path.unlink()
        resolved: list[str] = []
        sb.patch(
            build_seed=lambda: (SRC, IMAGE, USER_LO),
            so=lambda: resolved.append("so") or StubOracle(),
            run_elf=lambda *a, **k: b"",
        )
        rc = loop.main(["--advance"], {})
        check("main(--advance): blocked with no seed", rc, 2)
        check("main(--advance): the oracle was never resolved", resolved, [])


def case_main_advance_publishes_through_the_real_dispatch() -> None:
    """The orchestration a mocked `do_advance` call cannot see: argv -> mode ->
    build_seed -> do_advance -> published fixture."""
    with Sandbox() as sb:
        log: list[bytes] = []
        sb.patch(
            build_seed=lambda: (SRC, IMAGE, USER_LO),
            run_elf=stub_loop((NEW_POINT, NEW_POINT, NEW_POINT), log),
            stage0_emit=lambda c, u: NEW_POINT,
            so=lambda: StubOracle(),
        )
        rc = loop.main(["--advance"], {})
        check("main(--advance): publishes", rc, 0)
        check("main(--advance): the fixture holds the new point",
              sb.elf_path.read_bytes(), NEW_POINT)
        check("main(--advance): env form dispatches identically",
              loop.requested_modes([], {loop.ADVANCE_ENV: "1"})[0],
              [f"{loop.ADVANCE_ENV}=1"])


def case_main_reseed_dispatches_to_the_legacy_path() -> None:
    """--reseed still reaches its own mode and is not silently re-routed."""
    with Sandbox() as sb:
        seen: list[str] = []
        sb.patch(
            build_seed=lambda: (SRC, IMAGE, USER_LO),
            do_reseed=lambda c, i, u: seen.append("reseed") or 0,
            do_advance=lambda c, i, u: seen.append("advance") or 0,
        )
        check("main(--reseed): dispatches to reseed", loop.main(["--reseed"], {}), 0)
        check("main(--reseed): reached the legacy path", seen, ["reseed"])
        seen.clear()
        check("main(env reseed): dispatches to reseed",
              loop.main([], {loop.RESEED_ENV: "1"}), 0)
        check("main(env reseed): reached the legacy path", seen, ["reseed"])


def main() -> int:
    for fn in sorted(
        (v for k, v in globals().items() if k.startswith("case_")),
        key=lambda f: f.__name__,
    ):
        fn()
    if FAILURES:
        print(f"\n{len(FAILURES)} FAILED: {', '.join(FAILURES)}")
        return 1
    print("\nRESULT: PASS — --advance publishes only on full agreement, every "
          "ordinary refusal preserves the fixture, failed rollback is reported "
          "with recoverable copies, and ordinary verification is unweakened. "
          "(Harness controls: synthetic images, stub oracle — not "
          "compiler evidence.)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
