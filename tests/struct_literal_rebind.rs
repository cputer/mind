// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Inferred struct bindings keep aggregate identity across a same-type rebind.
//! A struct's opaque handle must not acquire the width of an integer literal.

use std::{fs, process::Command};

const PRELUDE: &str = r#"
struct Acc { count: i64, amount: i64 }
fn empty() -> Acc { return Acc { count: 0, amount: 0 }; }
fn add(a: Acc, x: i64) -> Acc {
    return Acc { count: a.count + 1, amount: a.amount + x };
}
fn wide() -> i64 { return 4294967296; }
"#;

fn check(body: &str, expected: Option<&str>) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("rebind.mind");
    fs::write(&source, format!("{PRELUDE}\n{body}")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_mindc"))
        .arg("check")
        .arg("--no-fmt")
        .arg("--no-lint")
        .arg(&source)
        .output()
        .unwrap();
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if let Some(code) = expected {
        assert!(!result.status.success(), "{body}\n{diagnostic}");
        assert!(diagnostic.contains(code), "{body}\n{diagnostic}");
        #[cfg(feature = "mlir-build")]
        {
            let artifact = dir.path().join("invalid.so");
            let build = Command::new(env!("CARGO_BIN_EXE_mindc"))
                .arg(&source)
                .arg("--emit-shared")
                .arg(&artifact)
                .output()
                .unwrap();
            let build_diagnostic = format!(
                "{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            );
            assert!(!build.status.success(), "{body}\n{build_diagnostic}");
            assert!(
                build_diagnostic.contains(code),
                "{body}\n{build_diagnostic}"
            );
            assert!(!artifact.exists(), "invalid source emitted an artifact");
        }
    } else {
        assert!(result.status.success(), "{body}\n{diagnostic}");
    }
}

#[test]
fn inferred_annotated_and_call_initialized_structs_accept_rebinding() {
    for initializer in [
        "let mut a = Acc { count: 0, amount: 0 };",
        "let mut a = ((Acc { count: 0, amount: 0 }));",
        "let mut a: Acc = Acc { count: 0, amount: 0 };",
        "let mut a = empty();",
        "let initial = Acc { count: 0, amount: 0 }; let mut a = initial;",
    ] {
        check(
            &format!(
                "pub fn run() -> i64 {{ {initializer} a = add(a, wide()); return a.amount + a.count; }}"
            ),
            None,
        );
    }
}

#[test]
fn actual_scalar_narrowing_still_refuses() {
    check(
        "pub fn run() -> i64 { let mut x: i32 = 0; x = wide(); return x; }",
        Some("E2004"),
    );
}

#[test]
fn scalar_values_cannot_replace_known_structs() {
    for initializer in [
        "let mut a = Acc { count: 0, amount: 0 };",
        "let mut a: Acc = empty();",
        "let mut a = empty();",
        "let initial = empty(); let mut a = initial;",
    ] {
        for value in ["1", "wide()", "1.5"] {
            check(
                &format!("pub fn run() -> i64 {{ {initializer} a = {value}; return 0; }}"),
                Some("E2026"),
            );
        }
    }
    check(
        "module nested { struct Other { amount: i64 } } pub fn run() -> i64 { let mut a = Other { amount: 0 }; a = wide(); return 0; }",
        Some("E2026"),
    );
}

#[test]
fn undeclared_literal_does_not_claim_known_struct_identity() {
    let source = "pub fn run() -> i64 { let mut a = Unknown { amount: 0 }; a = 2; return 0; }";
    let module = libmind::parser::parse(source).unwrap();
    let diagnostics =
        libmind::type_checker::check_module_types(&module, source, &Default::default());
    assert!(
        !diagnostics.iter().any(|d| d.code == "E2026"),
        "{diagnostics:?}"
    );
}

#[test]
fn lexical_scalar_shadow_does_not_inherit_struct_identity() {
    for body in [
        "let mut a: i64 = 0; a = wide();",
        "let (a, b) = (0, 1); a = 2;",
        "for a in 0..2 { a = 1; }",
    ] {
        check(
            &format!("pub fn run() -> i64 {{ let a = empty(); if true {{ {body} }} return 0; }}"),
            None,
        );
    }
}

#[test]
#[cfg(all(unix, feature = "std-surface", feature = "mlir-build"))]
fn native_rebind_preserves_full_width_and_replays_identically() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("native.mind");
    fs::write(
        &source,
        format!(
            r#"{PRELUDE}
pub fn inferred(x: i64) -> i64 {{
    let mut a = Acc {{ count: 0, amount: 0 }};
    a = add(a, x);
    return a.amount + a.count;
}}
pub fn annotated(x: i64) -> i64 {{
    let mut a: Acc = Acc {{ count: 0, amount: 0 }};
    a = add(a, x);
    return a.amount + a.count;
}}
pub fn initialized(x: i64) -> i64 {{
    let mut a = empty();
    a = add(a, x);
    return a.amount + a.count;
}}
"#
        ),
    )
    .unwrap();
    let artifacts = [dir.path().join("first.so"), dir.path().join("replay.so")];
    for artifact in &artifacts {
        let build = Command::new(env!("CARGO_BIN_EXE_mindc"))
            .arg(&source)
            .arg("--emit-shared")
            .arg(artifact)
            .output()
            .unwrap();
        assert!(
            build.status.success(),
            "{}{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
        let run = Command::new("python3")
            .arg("-c")
            .arg(
                r#"
import ctypes, sys
lib = ctypes.CDLL(sys.argv[1])
for name in ('inferred', 'annotated', 'initialized'):
    fn = getattr(lib, name)
    fn.argtypes = [ctypes.c_int64]
    fn.restype = ctypes.c_int64
    for value in (0, 41, 4294967296, -4294967296):
        got = fn(value)
        assert got == value + 1, (name, value, got)
"#,
            )
            .arg(artifact)
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
    }
    assert_eq!(
        fs::read(&artifacts[0]).unwrap(),
        fs::read(&artifacts[1]).unwrap()
    );
}
