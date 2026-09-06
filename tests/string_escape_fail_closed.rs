// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Unknown string/char escapes must FAIL CLOSED (issue #236).
//!
//! The parser used to keep the escaped byte and DROP the backslash, so
//! `"X\u{1f}Y"` silently became the 5-byte `"Xu{1f}Y"` — it type-checked, it
//! ran, and `mindc fmt` then wrote the corrupted literal back into the user's
//! source. A wrong string with zero diagnostics is the same silent-miscompile
//! class the lowering guards already refuse, so the parser now refuses it.
//!
//! Asserts three things: the unknown escape is REJECTED, every SUPPORTED
//! escape still decodes (no over-rejection), and `mindc fmt` no longer
//! rewrites the file.
//!
//! Gate: `cargo test --test string_escape_fail_closed`

#![cfg(unix)]

mod common;
use common::mindc_bin;

use std::process::Command;

const BAD: &str = "fn main() -> i64 {\n    let a = \"X\\u{1f}Y\"\n    return 0\n}\n";

// Every escape the language DOES support — none of these may be rejected.
const GOOD: &str =
    "fn main() -> i64 {\n    let a = \"n\\nt\\tr\\rz\\0b\\\\q\\'d\\\"e\"\n    return 0\n}\n";

#[test]
fn unknown_string_escape_is_rejected() {
    let mindc = mindc_bin();
    assert!(
        mindc.exists(),
        "string_escape_fail_closed requires the built mindc"
    );
    let src = common::scratch_dir("string_escape_fail_closed").join("mind_escape_bad.mind");
    std::fs::write(&src, BAD).expect("write src");

    let out = Command::new(&mindc)
        .args(["check", "--no-fmt", "--no-lint", src.to_str().unwrap()])
        .output()
        .expect("run mindc check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !out.status.success(),
        "an unknown escape must be REJECTED, not silently rewritten; got success:\n{combined}"
    );
    assert!(
        combined.contains("unknown string escape"),
        "diagnostic must name the unknown escape; got:\n{combined}"
    );
}

#[test]
fn supported_escapes_still_accepted() {
    let mindc = mindc_bin();
    assert!(
        mindc.exists(),
        "string_escape_fail_closed requires the built mindc"
    );
    let src = common::scratch_dir("string_escape_fail_closed").join("mind_escape_good.mind");
    std::fs::write(&src, GOOD).expect("write src");

    let out = Command::new(&mindc)
        .args(["check", "--no-fmt", "--no-lint", src.to_str().unwrap()])
        .output()
        .expect("run mindc check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "supported escapes must check successfully:\n{combined}"
    );
    assert!(
        !combined.contains("unknown string escape"),
        "the supported escape set must NOT be rejected (over-rejection regression):\n{combined}"
    );
}

#[test]
fn fmt_does_not_rewrite_an_unknown_escape() {
    let mindc = mindc_bin();
    assert!(
        mindc.exists(),
        "string_escape_fail_closed requires the built mindc"
    );
    let src = common::scratch_dir("string_escape_fail_closed").join("mind_escape_fmt.mind");
    std::fs::write(&src, BAD).expect("write src");

    let out = Command::new(&mindc)
        .args(["fmt", src.to_str().unwrap()])
        .output()
        .expect("run mindc fmt");

    assert!(
        !out.status.success(),
        "formatting an unknown escape must fail"
    );
    let after = std::fs::read_to_string(&src).expect("read back");
    assert_eq!(
        after, BAD,
        "mindc fmt must not rewrite a literal it cannot parse — that silently \
         corrupted the user's source"
    );
}
