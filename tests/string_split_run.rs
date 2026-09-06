// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! String method RUNTIME gate — `.split` / `.trim` (incl. through for-each).
//!
//! A method on a `String` receiver — an Ident bound to a string, a struct FIELD
//! of type `string`, or a for-each element typed from its collection — routes to
//! the `string_<method>` std free functions. `string_split(s, sep)` returns an
//! `array<string>` (a std.vec of String handles); `string_trim` strips ASCII
//! whitespace. This exercises the full chain mind-flow uses: a string struct
//! field `.split("+")`, a for-each over the split result (its elements typed as
//! String), and `.trim()` on each element.
//!
//! Gate: `cargo test --features "std-surface mlir-build cross-module-imports"
//!                   --test string_split_run`

#![cfg(all(
    unix,
    feature = "mlir-build",
    feature = "std-surface",
    feature = "cross-module-imports"
))]

mod common;
use common::{mindc_bin, scratch_dir};

use std::process::Command;

const SRC: &str = r#"
import std.string

struct Dec {
    arg_string: string,
}

// Split a string FIELD, iterate the result (elements typed String), trim each.
pub fn process(d: Dec) -> i64 {
    let mut n = 0
    let mut total = 0
    for part in d.arg_string.split("+") {
        let t = part.trim()
        n = n + 1
        total = total + string_len(t)
    }
    return n * 100 + total
}

fn mkstr(a: i64, b: i64, c: i64) -> string {
    let s = string_new()
    let s = string_push_byte(s, a)
    let s = string_push_byte(s, 43)
    let s = string_push_byte(s, b)
    let s = string_push_byte(s, 43)
    let s = string_push_byte(s, c)
    return s
}

pub fn run() -> i64 {
    let d = Dec { arg_string: mkstr(65, 66, 67) }
    return process(d)
}
"#;

const CALL_RESULT_SRC: &str = r#"
import std.string

fn make_one() -> string {
    let s = string_new()
    let s = string_push_byte(s, 65)
    return s
}

fn make_parts() -> string {
    let s = string_new()
    let s = string_push_byte(s, 65)
    let s = string_push_byte(s, 43)
    let s = string_push_byte(s, 66)
    return s
}

pub fn inferred_len() -> i64 {
    let s = make_one()
    return s.len()
}

pub fn direct_len() -> i64 {
    return make_one().len()
}

pub fn literal_len() -> i64 {
    return "abcd".len()
}

pub fn inferred_split() -> i64 {
    let s = make_parts()
    let mut total = 0
    for p in s.split("+") {
        total = total + string_len(p)
    }
    return total
}

pub fn direct_split() -> i64 {
    let mut total = 0
    for p in make_parts().split("+") {
        total = total + string_len(p)
    }
    return total
}

pub fn std_return_chain() -> i64 {
    let s = string_push_byte(string_new(), 65)
    return s.len()
}

pub fn lexical_shadow() -> i64 {
    let s = make_parts()
    if 1 == 1 {
        let s: i64 = 40
        if s == 40 { }
    }
    let mut total = 0
    for p in s.split("+") {
        total = total + string_len(p)
    }
    return total
}

pub fn run() -> i64 {
    return literal_len() * 1000000 + direct_len() * 100000
        + inferred_len() * 10000 + inferred_split() * 1000
        + direct_split() * 100 + std_return_chain() * 10 + lexical_shadow()
}
"#;

const SAME_NAME_USER_SRC: &str = r#"
import std.string

// A local declaration intentionally shadows the bundled std function name.
fn string_push_byte(s: string, b: i64) -> i64 {
    if string_len(s) == b {
        return 79
    }
    return 77
}

pub fn run() -> i64 {
    let s = string_new()
    return string_push_byte(s, 65)
}
"#;

fn compile_and_run(mindc: &std::path::Path, stem: &str, source: &str, expected: &[(&str, i64)]) {
    let dir = scratch_dir("string_split_run");
    let src = dir.join(format!("mind_{stem}.mind"));
    let so = dir.join(format!("mind_{stem}.so"));
    std::fs::write(&src, source).expect("write source");
    let out = Command::new(mindc)
        .args([src.to_str().unwrap(), "--emit-shared", so.to_str().unwrap()])
        .output()
        .expect("run mindc");
    assert!(
        out.status.success(),
        "{stem} compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(so.exists(), "{stem} must emit a shared library");
    let py = r#"import ctypes, json, sys
lib = ctypes.CDLL(sys.argv[1])
for name, expected in json.loads(sys.argv[2]):
    fn = getattr(lib, name)
    fn.restype = ctypes.c_int64
    actual = fn()
    assert actual == expected, f'{name}: {actual} != {expected}'
print('ok')
"#;
    let checks = serde_json::to_string(expected).expect("serialize expected results");
    let out = Command::new("python3")
        .args(["-c", py, so.to_str().unwrap(), &checks])
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "{stem} runtime check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

// mindc_bin() provided by tests/common (CARGO_BIN_EXE_mindc — staleness-free)

#[test]
fn string_split_runs() {
    let mindc = mindc_bin();
    if !mindc.exists() {
        crate::common::gate::skipped(
            "string_split_run",
            "string-split-run: mindc not found; skipping",
        );
        return;
    }
    let dir = scratch_dir("string_split_run");
    let src = dir.join("mind_string_split_run.mind");
    let so = dir.join("mind_string_split_run.so");
    std::fs::write(&src, SRC).expect("write src");

    let out = Command::new(&mindc)
        .args([src.to_str().unwrap(), "--emit-shared", so.to_str().unwrap()])
        .output()
        .expect("run mindc");
    if !crate::common::gate::compiled("string_split_run", &out) {
        return;
    }

    // "A+B+C".split("+") = 3 parts; each trims to length 1 → 3*100 + 3 = 303.
    let py = format!(
        "import ctypes\n\
         lib = ctypes.CDLL(r'{}')\n\
         lib.run.restype = ctypes.c_int64\n\
         r = lib.run(); assert r == 303, 'run=' + str(r)\n\
         print('ok')\n",
        so.to_string_lossy()
    );
    let out = Command::new("python3")
        .args(["-c", &py])
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "string-split-run check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn inferred_string_returns_and_shadowing_run() {
    let mindc = mindc_bin();
    if !mindc.exists() {
        panic!("string-call-result-metadata: mindc not found");
    }
    // literal_len=4, direct_len=1, inferred_len=1, inferred_split=2,
    // direct_split=2, std_return_chain=1, lexical_shadow=2.
    compile_and_run(
        &mindc,
        "string_call_result_metadata",
        CALL_RESULT_SRC,
        &[
            ("literal_len", 4),
            ("direct_len", 1),
            ("inferred_len", 1),
            ("inferred_split", 2),
            ("direct_split", 2),
            ("std_return_chain", 1),
            ("lexical_shadow", 2),
        ],
    );
    compile_and_run(
        &mindc,
        "string_same_name_user",
        SAME_NAME_USER_SRC,
        &[("run", 77)],
    );
}
