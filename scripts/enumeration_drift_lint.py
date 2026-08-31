#!/usr/bin/env python3
"""Fail closed when a comment's ENUMERATION drifts from the code it describes.

enforces: ENUM-DRIFT

WHY THIS EXISTS
---------------
A `let` with no single-token annotation stores 0 in the type slot. `parse_let`
documented that as "the exact no-annotation value every let consumer already
handles" and NAMED the handlers. `tc_let` was not among them, read
`ast_span_lo(0)`, and the process died at `node+8` — 21 times over four days.
The comment was true when written. Nothing re-checked it, so it became a lie
with a four-day fuse.

That defect class is a COMMENT THAT COUNTS OR NAMES A SET, sitting beside code
that owns the real set, with nothing comparing the two. Rust's exhaustiveness
checker does not help: an array literal's declared length is checked against its
own elements, never against the constants it was supposed to collect, and a
number written in prose is checked against nothing at all.

Two such drifts were measured 2026-08-30 and are pinned below. Both are the same
shape as the crash, and BOTH had already rotted:

  1. `src/parser/mod.rs::stmt_keyword` — the doc says the discriminator "maps the
     23-keyword set". The table holds 25. `trait` and `impl` were added to it on
     2026-07-30 (traits Phase 1); the prose was written 2026-07-14 and never
     bumped. 23 is the number of `(len, byte0)` CELLS, not keywords — so the
     stale number reads as plausible and survives review.

  2. `src/ir/compact/v3/evidence.rs` — a comment beside the read-side allowlist
     test said "the allowlist is exactly these five and nothing else" while
     `SIGNATURE_KEYS` held NINE. The four PQC keys (ml-dsa-87 / slh-dsa, pubkey
     and signature) had been added ten lines above a sentence still describing
     the five-key world. The *code* had already been fixed for this exact drift —
     the surviving sentence is the fossil of the bug it fixed.

WHAT IS CHECKED
---------------
  1. `stmt_keyword`'s prose set-size == its table's keyword count == the number
     of `StmtKw` enum variants. Catches a keyword added to the enum with no table
     arm (it would silently never parse) as well as a stale prose count.
  2. `SIGNATURE_KEYS` contains EVERY `const KEY_SIG_*` declared in evidence.rs,
     and `EVIDENCE_CHAIN_KEYS` contains every `evidence_chain.*` key const. This
     is the documented failure mode at the definition site: "a new `KEY_SIG_*`
     const being added without the reader learning about it". Proximity was the
     only mitigation; proximity is not enforcement. An unlisted key is rejected
     on read, so a producer emitting it would have every artifact fail to verify.
  3. No comment in either file states a set size that contradicts (1) or (2).

Each check carries a POSITIVE-COUNT guard: a parse that yields an empty set is a
failure, not a pass, so a refactor that breaks the parsing cannot turn this lint
into a green no-op.

Exit 0 = every enumeration agrees with the code that owns it. Exit 1 = drift, or
a parse that asserted nothing.
"""
from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

PARSER = ROOT / "src" / "parser" / "mod.rs"
EVIDENCE = ROOT / "src" / "ir" / "compact" / "v3" / "evidence.rs"

# Prose numbers are written as words as often as digits; both must be readable or
# a claim silently escapes the check.
_WORDS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10, "eleven": 11, "twelve": 12,
}


def _num(tok: str) -> int | None:
    tok = tok.strip().lower()
    if tok.isdigit():
        return int(tok)
    return _WORDS.get(tok)


def _read(path: pathlib.Path, problems: list[str]) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        problems.append(f"cannot read {path.relative_to(ROOT)}: {exc}")
        return ""


def check_stmt_keyword(problems: list[str]) -> str | None:
    """Prose count == table arms == enum variants."""
    src = _read(PARSER, problems)
    if not src:
        return None

    body = re.search(
        r"fn stmt_keyword\(w: &\[u8\]\) -> Option<StmtKw> \{(.*?)\n\}", src, re.S
    )
    if not body:
        problems.append(
            "could not locate `fn stmt_keyword` — this lint would assert nothing"
        )
        return None

    table = re.findall(r'\(b"(\w+)", StmtKw::(\w+)\)', body.group(1))
    words = sorted({w for w, _ in table})

    enum_block = re.search(r"enum StmtKw \{(.*?)\n\}", src, re.S)
    if not enum_block:
        problems.append("could not locate `enum StmtKw` — this lint would assert nothing")
        return None
    variants = sorted(
        {
            m.group(1)
            for line in enum_block.group(1).splitlines()
            if (m := re.match(r"\s*([A-Z]\w*)\s*,?\s*$", line))
        }
    )

    # Positive-count guard: an empty parse must fail, never pass.
    if len(words) < 2 or len(variants) < 2:
        problems.append(
            f"parsed {len(words)} table keywords / {len(variants)} StmtKw variants — "
            "a parse this small means the lint is asserting nothing"
        )
        return None

    if len(words) != len(variants):
        missing = sorted(set(variants) - {v for _, v in table})
        extra = sorted({v for _, v in table} - set(variants))
        problems.append(
            f"`stmt_keyword` recognises {len(words)} keywords but `StmtKw` declares "
            f"{len(variants)} variants"
            + (f"; no table arm produces {missing}" if missing else "")
            + (f"; table produces undeclared {extra}" if extra else "")
            + " — a variant with no arm is a keyword that silently never parses."
        )

    # The prose set-size claim in the doc comment above the fn.
    doc = src[: body.start()]
    claim = re.search(r"maps the ([\w-]+)-keyword set", doc)
    if not claim:
        problems.append(
            "the `stmt_keyword` doc no longer states an N-keyword set size — this lint "
            "pins that sentence; restore it or drop this check deliberately"
        )
    else:
        stated = _num(claim.group(1))
        if stated is None:
            problems.append(f"unparseable keyword-set size in prose: {claim.group(1)!r}")
        elif stated != len(words):
            problems.append(
                f"`src/parser/mod.rs` doc says the discriminator maps the "
                f"{stated}-keyword set; the table holds {len(words)}. NOTE: the "
                f"{len(set((len(w), w[0]) for w in words))} `(len, byte0)` CELLS is a "
                "different number — do not 'fix' the prose with the cell count."
            )
    return f"stmt_keyword: {len(words)} keywords, {len(variants)} StmtKw variants agree"


def _array_elems(src: str, name: str) -> list[str] | None:
    m = re.search(rf"const {name}: \[&str; \d+\] = \[(.*?)\];", src, re.S)
    if not m:
        return None
    return [e.strip() for e in m.group(1).split(",") if e.strip()]


def check_evidence_key_allowlists(problems: list[str]) -> str | None:
    """Every reserved-namespace key const must appear in its read-side allowlist."""
    src = _read(EVIDENCE, problems)
    if not src:
        return None

    out = []
    for array, const_re, human in (
        ("SIGNATURE_KEYS", r"^const (KEY_SIG_\w+): &str", "signature."),
        (
            "EVIDENCE_CHAIN_KEYS",
            r'^const (KEY_\w+): &str = "evidence_chain\.',
            "evidence_chain.",
        ),
    ):
        elems = _array_elems(src, array)
        if elems is None:
            problems.append(f"could not locate `const {array}` — lint asserts nothing")
            continue
        consts = re.findall(const_re, src, re.M)

        # Positive-count guard.
        if len(elems) < 2 or len(consts) < 2:
            problems.append(
                f"parsed {len(elems)} {array} entries / {len(consts)} `{human}` consts "
                "— a parse this small means the lint is asserting nothing"
            )
            continue

        unlisted = sorted(set(consts) - set(elems))
        if unlisted:
            problems.append(
                f"{unlisted} declared as `{human}` key const(s) but absent from "
                f"`{array}` — `parse_map_epilogue` REJECTS any key not in that array, "
                "so every artifact carrying one would fail verification on read."
            )
        stale = sorted(set(elems) - set(consts))
        if stale:
            problems.append(f"`{array}` lists {stale}, which no longer exist")
        out.append(f"{array}: {len(elems)} entries cover all {len(consts)} consts")

    # A prose set-size claim about either allowlist must match it.
    for claim in re.finditer(r"allowlist is exactly these ([\w-]+)\b", src):
        stated = _num(claim.group(1))
        actual = _array_elems(src, "SIGNATURE_KEYS")
        if stated is not None and actual is not None and stated != len(actual):
            line = src[: claim.start()].count("\n") + 1
            problems.append(
                f"src/ir/compact/v3/evidence.rs:{line} says the allowlist is "
                f'"exactly these {claim.group(1)}"; `SIGNATURE_KEYS` holds {len(actual)}'
            )
    return "; ".join(out) if out else None


def main() -> int:
    problems: list[str] = []
    notes = [
        n
        for n in (
            check_stmt_keyword(problems),
            check_evidence_key_allowlists(problems),
        )
        if n
    ]

    if problems:
        print("FAIL: a comment's enumeration disagrees with the code it describes.\n",
              file=sys.stderr)
        for p in problems:
            print(f"  - {p}", file=sys.stderr)
        print(
            "\nThis is the tc_let shape: a comment that NAMES or COUNTS a set, beside "
            "code that owns the real set, with nothing comparing the two. Fix the side "
            "that is wrong — do not delete the claim to silence the lint.",
            file=sys.stderr,
        )
        return 1

    for n in notes:
        print(f"PASS: {n}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
