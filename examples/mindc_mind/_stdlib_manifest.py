"""Shared reader for the committed std/*.mind manifest.

enforced-by: STDLIB-MANIFEST

Why this exists
---------------
"Which std modules does the native backend link" used to live as SEVENTEEN
hand-copied literal lists — one in ``src/bin/mindc.rs::run_native_backend_bridge``
and sixteen ``_STDLIB_MODULES`` / ``STD_MODULES`` copies across
``examples/mindc_mind/*.py`` — plus a fourth, DIFFERENT list
(``STDLIB_MIND_SOURCES``, 23 entries) in ``src/project/stdlib.rs``, with nothing
asserting any of them agreed. Two hand-copied lists that must agree, with no check,
is the most common defect class in this tree.

``testdata/stdlib_manifest.txt`` is the one committed contract. This module is the
Python reader; ``mindc.rs::frozen_std_seed_modules`` is the Rust reader, and it
parses the same file with the same rules (``include_str!`` at compile time, so the
shipping binary keeps no runtime file dependency).
``stdlib_manifest_lint.py`` gates the whole thing in both directions.

ORDER IS LOAD-BEARING: ``seed_modules()`` returns the seed rows in FILE ORDER,
which is the byte order of the std blob the frozen ``stage1.elf`` was minted from.
Reordering, adding or removing a seed row changes the compiled bytes of every
native build — a whole-corpus RESEED event, never a routine edit.
"""

import hashlib
import pathlib
import re

_HERE = pathlib.Path(__file__).parent.resolve()
_REPO = _HERE.parents[1]

MANIFEST_PATH = _HERE / "testdata" / "stdlib_manifest.txt"
STD_DIR = _REPO / "std"

# Kept in lockstep with mindc.rs::FROZEN_STD_SEED_COUNT. Both are deliberate
# tripwires, not redundancy: the seed set cannot change without a stage1.elf
# re-mint, so a quiet one-line manifest edit must fail loudly on both sides.
FROZEN_STD_SEED_COUNT = 21


def _rows():
    """(module, native_seed, bundled) for every manifest row, in file order.

    Parsing rules are the exact twin of ``mindc.rs::frozen_std_seed_modules``:
    drop ``#`` comment lines and blank lines, split the rest on TAB.
    """
    out = []
    text = MANIFEST_PATH.read_text(encoding="utf-8")
    for lineno, line in enumerate(text.splitlines(), 1):
        if line.startswith("#") or not line.strip():
            continue
        fields = line.split("\t")
        if len(fields) != 3:
            raise ValueError(
                f"{MANIFEST_PATH}:{lineno}: expected 3 tab-separated fields, got "
                f"{len(fields)}: {line!r}"
            )
        module, native_seed, bundled = fields
        if native_seed not in ("seed", "no"):
            raise ValueError(
                f"{MANIFEST_PATH}:{lineno}: native_seed must be 'seed' or 'no', "
                f"got {native_seed!r}"
            )
        if bundled not in ("bundled", "no"):
            raise ValueError(
                f"{MANIFEST_PATH}:{lineno}: bundled must be 'bundled' or 'no', "
                f"got {bundled!r}"
            )
        out.append((module, native_seed, bundled))
    if not out:
        raise ValueError(f"{MANIFEST_PATH}: no rows — refusing to return an empty std set")
    return out


def all_modules():
    """Every std module the manifest knows about, in file order."""
    return [m for m, _, _ in _rows()]


def seed_modules():
    """The std blob the native bridge feeds to the frozen stage1.elf, IN ORDER."""
    return [m for m, seed, _ in _rows() if seed == "seed"]


def bundled_modules():
    """The modules ``STDLIB_MIND_SOURCES`` resolves for ``use std.<m>``."""
    return [m for m, _, bundled in _rows() if bundled == "bundled"]


def seed_blob(std_dir=None):
    """The exact std-blob prefix of the stage1.elf stdin image.

    Byte-identical to the Rust bridge's composition: each module's bytes followed
    by a single ``\\n``.
    """
    d = pathlib.Path(std_dir) if std_dir is not None else STD_DIR
    return b"".join((d / f"{m}.mind").read_bytes() + b"\n" for m in seed_modules())


def declared_seed_sha256():
    """The ``SEED_BLOB_SHA256`` golden recorded in the manifest header, or None.

    The lint recomputes the blob and compares; the golden is a positive pin on the
    seed set AND its order, so a reseed cannot land as a silent edit.
    """
    text = MANIFEST_PATH.read_text(encoding="utf-8")
    m = re.search(r"^#\s*SEED_BLOB_SHA256\s+([0-9a-f]{64})\s*$", text, re.M)
    return m.group(1) if m else None


def actual_seed_sha256(std_dir=None):
    """sha256 of the blob composed from the manifest + the std/ files on disk."""
    return hashlib.sha256(seed_blob(std_dir)).hexdigest()
