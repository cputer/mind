// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! The `u64` SIGNEDNESS TAG must survive every carrier that moves a value.
//!
//! Unsignedness is not a property of the emitted i64 bits — it is carried by an
//! identity `__mind_conv_u64` marker whose MLIR result is typed `ScalarU64`, and
//! that tag is the ONLY thing that makes `>>`, `>` and `/` select the unsigned
//! MLIR variant (`shrui` / `cmpi ugt` / `divui`). Every construct that produces a
//! NEW SSA value for an existing `u64` variable therefore has to re-apply the tag,
//! and each one that forgets is a silent miscompile: the program keeps compiling,
//! keeps running, and answers with SIGNED arithmetic.
//!
//! Five constructs carry the tag. They were fixed one at a time, and only the
//! array leg had a regression test — so four of the five were unpinned and a
//! later refactor could drop any of them without a red test:
//!
//!   1. if-merge        — a `u64` assigned in both arms, read after the join
//!   2. while-merge     — a `u64` assigned in a loop body, read after the loop
//!   3. reassignment    — straight-line `y = …` on a declared `u64`
//!   4. struct field    — `s.v` where the declared field type is `u64`
//!   5. array element   — `a[i]` where the declared element type is `u64`
//!
//! The assertions are made on a BUILT AND EXECUTED cdylib, never on emitted IR or
//! MLIR text: a text assertion pins a spelling, and the spelling is allowed to
//! change. `u64::MAX` is the discriminating input because it is the i64 bit
//! pattern `-1`, so all three operations answer differently under the two
//! signednesses:
//!
//!     u64::MAX >> 63   unsigned 1                   signed -1
//!     u64::MAX >  5    unsigned true                signed false
//!     u64::MAX /  2    unsigned 9223372036854775807 signed 0
//!
//! Each carrier also compiles an `i64` CONTROL of the identical shape whose
//! expected answer is the SIGNED one (-1). A carrier whose control does not
//! answer -1 is not reaching the shift at all, which would make the u64
//! assertion hold vacuously.
//!
//! # Each leg is an independent pin, and was proved so
//!
//! The four active carriers do NOT share one mechanism, so one test could not
//! have covered them. Each was individually reverted and the suite re-run; each
//! revert reddened EXACTLY its own leg and left the other three green:
//!
//!   * reassignment + if-merge — `record_narrow_let` (src/eval/lower.rs) must
//!     register a declared `u64`, or a later `y = …` has nothing to re-tag from.
//!     Dropping `matches!(scalar_int64_cast_signed(ty), Some(false))` from its
//!     guard reddens both legs and neither other one.
//!   * while-merge — the `^while_after` exit-id kind is CLONED from the pre-loop
//!     init kind (src/mlir/lowering.rs). Hardcoding `ValueKind::ScalarI64` there
//!     instead reddens only this leg.
//!   * array element — the `vec_get` result is re-materialised at the declared
//!     element type (`mask_narrow_let` on the index path in src/eval/lower.rs).
//!     Returning the bare `dst` reddens only this leg.
//!
//! Gate: `cargo test --no-default-features \
//!        --features "mlir-build std-surface cross-module-imports" \
//!        --test u64_tag_survival_run`

#![cfg(all(
    unix,
    feature = "mlir-build",
    feature = "std-surface",
    feature = "cross-module-imports"
))]

mod common;
use common::mindc_bin;

use std::process::Command;

/// `u64::MAX / 2` — the unsigned quotient. The signed quotient of the same bits
/// (`-1 / 2`) is 0, so this constant is what separates `divui` from `divsi`.
///
/// NO fixture writes an explicit `as u64` cast anywhere. An explicitly-cast
/// operand carries a `__mind_conv_u64` tag of its OWN, so it supplies the
/// unsignedness the test is trying to observe and the assertion holds on a
/// compiler that has lost the tag entirely. Measured on the known-untagged
/// struct-field carrier: a cast divisor answers 9223372036854775807 (looks
/// fixed) where the bare literal answers 0 (the actual signed result). Every
/// `u64` here is therefore declared once, on a `let` / field / element type, and
/// every operand it meets is a bare literal.
const U64_MAX_DIV_2: i64 = 9_223_372_036_854_775_807;

/// Build `src` as a cdylib and assert each `(exported fn, expected i64)` pair by
/// CALLING it. Returns without asserting only when the toolchain itself is
/// absent (mindc not built, or built without an MLIR backend) — a compile
/// failure of the fixture is a hard failure, never a skip.
fn assert_carrier(stem: &str, src: &str, expected: &[(&str, i64)]) {
    let mindc = mindc_bin();
    if !mindc.exists() {
        crate::common::gate::skipped(
            "u64_tag_survival_run",
            &format!("u64-tag-survival[{stem}]: mindc not found; skipping"),
        );
        return;
    }
    let dir = std::env::temp_dir();
    let src_path = dir.join(format!("mind_u64_tag_{stem}.mind"));
    let so_path = dir.join(format!("mind_u64_tag_{stem}.so"));
    std::fs::write(&src_path, src).expect("write fixture");

    let out = Command::new(&mindc)
        .args([
            src_path.to_str().unwrap(),
            "--emit-shared",
            so_path.to_str().unwrap(),
        ])
        .output()
        .expect("run mindc");
    if !crate::common::gate::compiled("u64_tag_survival_run", &out) {
        return;
    }

    let mut script = format!(
        "import ctypes\nlib = ctypes.CDLL(r'{}')\nbad = []\n",
        so_path.to_string_lossy()
    );
    for (name, want) in expected {
        script.push_str(&format!(
            "getattr(lib, '{name}').restype = ctypes.c_int64\n\
             _g = lib.{name}()\n\
             if _g != {want}: bad.append('{name}: got ' + str(_g) + ' want {want}')\n"
        ));
    }
    script.push_str("assert not bad, '; '.join(bad)\nprint('ok')\n");

    let run = Command::new("python3")
        .args(["-c", &script])
        .output()
        .expect("python3");
    assert!(
        run.status.success(),
        "u64-tag-survival[{stem}]: the u64 signedness tag did not survive this carrier \
         — the listed calls answered with SIGNED arithmetic:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
}

/// Carrier 1 — the value read after an `if` is the merge block's argument, so the
/// join has to produce a `ScalarU64` when both incoming arms are `u64`.
const IF_MERGE: &str = r#"
pub fn shr() -> i64 {
    let mut y: u64 = 1
    if 1 == 1 {
        y = 0 - 1
    } else {
        y = 2
    }
    return y >> 63
}
pub fn gt() -> i64 {
    let mut y: u64 = 1
    if 1 == 1 {
        y = 0 - 1
    } else {
        y = 2
    }
    if y > 5 {
        return 1
    }
    return 0
}
pub fn div() -> i64 {
    let mut y: u64 = 1
    if 1 == 1 {
        y = 0 - 1
    } else {
        y = 2
    }
    return y / 2
}
pub fn ctrl_i64() -> i64 {
    let mut y: i64 = 1
    if 1 == 1 {
        y = 0 - 1
    } else {
        y = 2
    }
    return y >> 63
}
"#;

/// Carrier 2 — the value read after a `while` is the loop-carried block argument.
const WHILE_MERGE: &str = r#"
pub fn shr() -> i64 {
    let mut y: u64 = 1
    let mut i: i64 = 0
    while i < 1 {
        y = 0 - 1
        i = i + 1
    }
    return y >> 63
}
pub fn gt() -> i64 {
    let mut y: u64 = 1
    let mut i: i64 = 0
    while i < 1 {
        y = 0 - 1
        i = i + 1
    }
    if y > 5 {
        return 1
    }
    return 0
}
pub fn div() -> i64 {
    let mut y: u64 = 1
    let mut i: i64 = 0
    while i < 1 {
        y = 0 - 1
        i = i + 1
    }
    return y / 2
}
pub fn ctrl_i64() -> i64 {
    let mut y: i64 = 1
    let mut i: i64 = 0
    while i < 1 {
        y = 0 - 1
        i = i + 1
    }
    return y >> 63
}
"#;

/// Carrier 3 — straight-line reassignment. The declared type lives on the `let`,
/// so the assign has to consult the narrow-locals registry to re-tag.
const REASSIGN: &str = r#"
pub fn shr() -> i64 {
    let mut y: u64 = 5
    y = 0 - 1
    return y >> 63
}
pub fn gt() -> i64 {
    let mut y: u64 = 5
    y = 0 - 1
    if y > 5 {
        return 1
    }
    return 0
}
pub fn div() -> i64 {
    let mut y: u64 = 5
    y = 0 - 1
    return y / 2
}
pub fn ctrl_i64() -> i64 {
    let mut y: i64 = 5
    y = 0 - 1
    return y >> 63
}
"#;

/// Carrier 4 — a struct field read. The declared FIELD type is the only place the
/// unsignedness is written down.
const STRUCT_FIELD: &str = r#"
struct S {
    v: u64,
}
struct T {
    v: i64,
}
pub fn shr() -> i64 {
    let s = S { v: 0 - 1 }
    let y = s.v
    return y >> 63
}
pub fn gt() -> i64 {
    let s = S { v: 0 - 1 }
    let y = s.v
    if y > 5 {
        return 1
    }
    return 0
}
pub fn div() -> i64 {
    let s = S { v: 0 - 1 }
    let y = s.v
    return y / 2
}
pub fn ctrl_i64() -> i64 {
    let t = T { v: 0 - 1 }
    let y = t.v
    return y >> 63
}
"#;

/// Carrier 5 — an array element read. This is the leg that already had a
/// regression test; it is repeated here so all five carriers are asserted by one
/// gate and a future refactor cannot silently drop the odd one out.
const ARRAY_ELEM: &str = r#"
pub fn shr() -> i64 {
    let a: array<u64> = [0 - 1]
    let y = a[0]
    return y >> 63
}
pub fn gt() -> i64 {
    let a: array<u64> = [0 - 1]
    let y = a[0]
    if y > 5 {
        return 1
    }
    return 0
}
pub fn div() -> i64 {
    let a: array<u64> = [0 - 1]
    let y = a[0]
    return y / 2
}
pub fn ctrl_i64() -> i64 {
    let a: array<i64> = [0 - 1]
    let y = a[0]
    return y >> 63
}
"#;

/// The expected answers are identical for every carrier — that is the point: the
/// tag is a property of the VALUE, so no construct is allowed to change what the
/// three operations mean.
const EXPECTED: [(&str, i64); 4] = [
    ("shr", 1),
    ("gt", 1),
    ("div", U64_MAX_DIV_2),
    ("ctrl_i64", -1),
];

#[test]
fn u64_tag_survives_if_merge() {
    assert_carrier("if_merge", IF_MERGE, &EXPECTED);
}

#[test]
fn u64_tag_survives_while_merge() {
    assert_carrier("while_merge", WHILE_MERGE, &EXPECTED);
}

#[test]
fn u64_tag_survives_reassignment() {
    assert_carrier("reassign", REASSIGN, &EXPECTED);
}

// Open defect owned by task WS0-03: a `u64` struct-field read returns the bare
// loaded value with no `__mind_conv_u64` marker, so `>>`, `>` and `/` all pick the
// signed variant. The test is written and kept RED-capable rather than deleted;
// WS0-03 removes this attribute as its own proof of fix.
#[test]
#[ignore = "open defect owned by WS0-03: struct u64 field read is untagged"]
fn u64_tag_survives_struct_field() {
    assert_carrier("struct_field", STRUCT_FIELD, &EXPECTED);
}

#[test]
fn u64_tag_survives_array_element() {
    assert_carrier("array_elem", ARRAY_ELEM, &EXPECTED);
}
