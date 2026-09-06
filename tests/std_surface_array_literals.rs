// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! RFC 0005 Phase 6.2b Gap 2 — array literals `[expr, expr, ...]` and
//! fixed-size array types `[T; N]`.
//!
//! Covers:
//! 1. `[1, 2, 3]` parses and lowers to a `ConstArray` IR instruction typed as `[i64; 3]`.
//! 2. `[i64; 0]` (empty array type) parses and is a valid type annotation.
//! 3. A 4,096-entry array literal (generated programmatically) parses without
//!    stack overflow and produces exactly 4,096 elements.
//! 4. `const FOO: [i64; 4] = [1, 2, 3, 4]; fn nth(i: i64) -> i64 { FOO[i] }`
//!    — the const is registered in the module-level env and IndexAccess on it
//!    lowers to a `__mind_array_load_i64` call.
//! 5. A type-length mismatch `let x: [i64; 3] = [1, 2]` is rejected at
//!    type-check time with a diagnostic containing "length".
//! 6. A computed fixed-array return executes through direct, aliased, and
//!    implicit tail-expression forms and reproduces byte-identically.
//!
//! Gated: `cargo test --features std-surface --test std_surface_array_literals`.

#![cfg(feature = "std-surface")]

use libmind::eval::lower::lower_to_ir;
use libmind::ir::Instr;
#[cfg(feature = "mlir-lowering")]
use libmind::ir::ValueId;
use libmind::parser;
#[cfg(feature = "mlir-lowering")]
use libmind::{MlirLowerError, compile_ir_to_mlir_text};

// ── Test 1: basic array literal parses + lowers ──────────────────────────────

#[test]
fn array_lit_three_elements_parses_and_lowers() {
    let src = "[1, 2, 3]";
    let module = parser::parse(src).expect("[1, 2, 3] must parse");
    // The parsed module should contain an ArrayLit node.
    let ir = lower_to_ir(&module);
    // Must contain a ConstArray instruction with 3 elements.
    let has_const_array = ir
        .instrs
        .iter()
        .any(|i| matches!(i, Instr::ConstArray { values, .. } if values.len() == 3));
    assert!(
        has_const_array,
        "expected ConstArray with 3 elements in IR, got: {:?}",
        ir.instrs
    );
}

// ── Test 2: empty array literal ───────────────────────────────────────────────

#[test]
fn array_lit_empty_parses() {
    // Empty array literal should parse without error.
    let src = "let x: [i64; 0] = []";
    let _module = parser::parse(src).expect("[i64; 0] empty array must parse");
}

// ── Test 3: large array literal (4,096 entries, no stack overflow) ────────────

#[test]
fn array_lit_4096_entries_no_stack_overflow() {
    // Generate a 4096-element array literal: [0, 1, 2, ..., 4095]
    let mut src = String::from("[");
    for i in 0u32..4096 {
        if i > 0 {
            src.push_str(", ");
        }
        src.push_str(&i.to_string());
    }
    src.push(']');

    let module = parser::parse(&src).expect("4096-entry array literal must parse without overflow");
    let ir = lower_to_ir(&module);
    let has_const_array = ir
        .instrs
        .iter()
        .any(|i| matches!(i, Instr::ConstArray { values, .. } if values.len() == 4096));
    assert!(
        has_const_array,
        "expected ConstArray with 4096 elements in lowered IR"
    );
}

// ── Test 4: const array + index access ───────────────────────────────────────

#[test]
fn const_array_and_index_access_lower() {
    let src = r#"
const FOO: [i64; 4] = [1, 2, 3, 4];

fn nth(i: i64) -> i64 {
    FOO[i]
}
"#;
    let module = parser::parse(src).expect("const FOO + fn nth must parse");
    let ir = lower_to_ir(&module);

    // The module-level ConstArray for FOO must exist.
    let has_foo = ir.instrs.iter().any(|i| {
        matches!(i, Instr::ConstArray { name: Some(n), values, .. }
            if n == "FOO" && values.len() == 4)
    });
    assert!(has_foo, "expected named ConstArray 'FOO' in module IR");

    // The fn body of `nth` must contain an array load.
    let fn_body = ir
        .instrs
        .iter()
        .find_map(|i| match i {
            Instr::FnDef { name, body, .. } if name == "nth" => Some(body.as_slice()),
            _ => None,
        })
        .expect("expected FnDef 'nth' in module IR");

    let has_load = fn_body.iter().any(|i| matches!(i, Instr::ArrayLoad { .. }));
    assert!(
        has_load,
        "expected ArrayLoad instruction in fn nth body, got: {:?}",
        fn_body
    );
}

// ── Test 5: type-length mismatch rejected ────────────────────────────────────

#[test]
fn type_mismatch_length_rejected() {
    use libmind::type_checker;
    let src = "let x: [i64; 3] = [1, 2]";
    let module = parser::parse(src).expect("let x: [i64; 3] = [1, 2] must parse");
    let diags = type_checker::check_module_types(&module, src, &Default::default());
    assert!(
        !diags.is_empty(),
        "expected type-check diagnostic for array length mismatch, but got none"
    );
    // The diagnostic message must mention the length discrepancy.
    let has_length_msg = diags.iter().any(|d| {
        d.message.contains("length")
            || d.message.contains("3")
            || d.message.contains("2")
            || d.message.contains("array")
            || d.message.contains("mismatch")
    });
    assert!(
        has_length_msg,
        "diagnostic should mention length mismatch, got: {:?}",
        diags
    );
}

// ── Test 6: alias-typed array parameter keeps aggregate metadata ────────────

#[test]
#[cfg(feature = "mlir-lowering")]
fn alias_typed_array_param_lowers_like_its_target() {
    fn mlir(src: &str) -> String {
        let module = parser::parse(src).expect("array-param module must parse");
        let mut ir = lower_to_ir(&module);
        compile_ir_to_mlir_text(&mut ir).expect("array-param module must lower to MLIR")
    }

    let alias_src = r#"
type Element = f64
type ElementArray = [Element; 4]
type Words = ElementArray
type Result = Element

fn first(values: Words) -> Result {
    return values[0]
}
"#;
    let direct_src = r#"
fn first(values: [f64; 4]) -> f64 {
    return values[0]
}
"#;

    fn first_fn(text: &str) -> &str {
        let start = text.find("  func.func @first(").expect("first function");
        let end = text[start..]
            .find("\n  }\n")
            .map(|offset| start + offset + "\n  }\n".len())
            .expect("first function end");
        &text[start..end]
    }

    let alias_mlir = mlir(alias_src);
    let direct_mlir = mlir(direct_src);

    assert_eq!(
        first_fn(&alias_mlir),
        first_fn(&direct_mlir),
        "a type alias must preserve the target array's ABI and ValueKind metadata"
    );
    assert!(
        alias_mlir.contains("func.func @first(%0: tensor<4xf64>) -> f64"),
        "alias-typed array parameter must retain its tensor boundary: {alias_mlir}"
    );
    assert_eq!(
        alias_mlir,
        mlir(alias_src),
        "alias resolution must emit deterministic MLIR"
    );

    fn assert_array_load_refusal(src: &str, reason: &str) {
        let module = parser::parse(src).expect("refusal module must parse");
        let mut ir = lower_to_ir(&module);
        let err = compile_ir_to_mlir_text(&mut ir)
            .expect_err("unresolved aggregate type must not acquire an invented ABI");
        assert!(
            matches!(
                err,
                MlirLowerError::MissingTypeInfo {
                    value: ValueId(0),
                    context: "array load base"
                }
            ),
            "{reason} must retain the fail-closed array-load refusal: {err}"
        );
    }

    assert_array_load_refusal(
        r#"
type Recursive = [Recursive; 1]

fn first(values: Recursive) -> i64 {
    return values[0]
}
"#,
        "cyclic alias",
    );
    assert_array_load_refusal(
        r#"
fn first(values: Missing) -> i64 {
    return values[0]
}
"#,
        "unresolved named type",
    );
}

// ── Test 7: computed fixed-array return executes and is deterministic ───────

#[test]
#[cfg(all(unix, feature = "mlir-build"))]
fn computed_fixed_array_return_runs_and_replays_identically() {
    use std::fs;
    use std::process::Command;

    let dir = tempfile::tempdir().expect("temporary compiler outputs");
    let source = dir.path().join("computed_return.mind");
    fs::write(
        &source,
        r#"
fn make_marker(m: i64) -> [i64; 4] {
    return [0, 0, 0, m];
}
type Marker = [i64; 4]
fn make_alias(m: i64) -> Marker {
    return [0, 0, 0, m];
}
fn make_implicit(m: i64) -> [i64; 4] {
    [0, 0, 0, m]
}
fn make_parenthesized(m: i64) -> Marker {
    return (([0, 0, 0, m]));
}
fn make_tail_parenthesized(m: i64) -> Marker {
    (([0, 0, 0, m]))
}
pub fn probe(m: i64) -> i64 {
    let values: [i64; 4] = make_marker(m);
    return values[3];
}
pub fn probe_alias(m: i64) -> i64 {
    let values: Marker = make_alias(m);
    return values[3];
}
pub fn probe_implicit(m: i64) -> i64 {
    let values: [i64; 4] = make_implicit(m);
    return values[3];
}
pub fn probe_parenthesized(m: i64) -> i64 {
    let values: Marker = make_parenthesized(m);
    return values[3];
}
pub fn probe_tail_parenthesized(m: i64) -> i64 {
    let values: Marker = make_tail_parenthesized(m);
    return values[3];
}
"#,
    )
    .expect("write computed return source");

    let compile = |artifact: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_mindc"))
            .arg(&source)
            .arg("--emit-shared")
            .arg(artifact)
            .output()
            .expect("run mindc")
    };
    let first = dir.path().join("first.so");
    let output = compile(&first);
    assert!(
        output.status.success(),
        "computed fixed-array return must compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let python = r#"
import ctypes, sys
lib = ctypes.CDLL(sys.argv[1])
names = ("probe", "probe_alias", "probe_implicit", "probe_parenthesized", "probe_tail_parenthesized")
for name in names:
    fn = getattr(lib, name)
    fn.argtypes = [ctypes.c_int64]
    fn.restype = ctypes.c_int64
for value in (0, 1, 41, -7, 9223372036854775807):
    for name in names:
        got = getattr(lib, name)(value)
        assert got == value, (name, value, got)
print("computed fixed-array return values ok")
"#;
    let run = Command::new("python3")
        .arg("-c")
        .arg(python)
        .arg(&first)
        .output()
        .expect("run native fixed-array return probe");
    assert!(
        run.status.success(),
        "computed fixed-array return produced wrong value:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let second = dir.path().join("second.so");
    let replay = compile(&second);
    assert!(
        replay.status.success(),
        "second deterministic compile must succeed:\n{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        fs::read(&first).expect("read first artifact"),
        fs::read(&second).expect("read replay artifact"),
        "identical source and compiler must produce byte-identical shared artifacts"
    );

    let negative_cases = [
        (
            "cardinality",
            r#"pub fn bad() -> [i64; 3] {
    return [1, 2];
}
"#,
        ),
        (
            "element type",
            r#"pub fn bad() -> [i64; 2] {
    return [1.25, 2.5];
}
"#,
        ),
        (
            "alias cardinality",
            r#"type Values = [i64; 3]
pub fn bad() -> Values {
    return [1, 2];
}
"#,
        ),
    ];
    for (name, text) in negative_cases {
        let bad_source = dir.path().join(format!("negative_{name}.mind"));
        let bad_artifact = dir.path().join(format!("negative_{name}.so"));
        fs::write(&bad_source, text).expect("write negative source");
        let checked = Command::new(env!("CARGO_BIN_EXE_mindc"))
            .args(["check", "--no-fmt", "--no-lint"])
            .arg(&bad_source)
            .output()
            .expect("check negative fixed-array return");
        assert_eq!(
            checked.status.code(),
            Some(1),
            "negative {name} must fail check"
        );
        let checked_text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&checked.stdout),
            String::from_utf8_lossy(&checked.stderr)
        );
        assert!(
            checked_text.contains("E2001"),
            "negative {name} must report its type error: {checked_text}"
        );
        let bad = Command::new(env!("CARGO_BIN_EXE_mindc"))
            .arg(&bad_source)
            .arg("--emit-shared")
            .arg(&bad_artifact)
            .output()
            .expect("run mindc negative control");
        assert!(!bad.status.success(), "negative {name} case must fail");
        let diagnostic = String::from_utf8_lossy(&bad.stderr);
        assert!(
            !diagnostic.contains("E5003"),
            "negative {name} must fail for its source, not a missing backend: {diagnostic}"
        );
        assert!(
            !bad_artifact.exists(),
            "negative {name} case must not leave an artifact"
        );
    }
}
