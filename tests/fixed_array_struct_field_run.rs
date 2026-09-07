// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Executable MLIR gate for fixed arrays stored in struct fields.
//!
//! The test deliberately emits and loads a real shared artifact.  The
//! interpreter/check path alone cannot catch an aggregate field being lowered
//! as an opaque i64 record slot.

#![cfg(all(unix, feature = "mlir-build", feature = "std-surface"))]

mod common;

use libloading::{Library, Symbol};
use libmind::eval::lower::lower_to_ir;
use libmind::ir::Instr;
use libmind::parser;
use std::process::Command;

const SOURCE: &str = r#"
type Elem = i64
type Words = [Elem; 2]
type Four = [i64; 4]
type TwoWords = [i64; 2]
type ThreeWords = [i64; 3]

struct S { xs: [i64; 4], tail: i64 }
struct AliasS { xs: Four }
struct F { xs: [f64; 2] }
struct Inner { xs: [i64; 2] }
struct Outer { inner: Inner }
struct One { xs: [i64; 1], tail: i64 }
struct Empty { xs: [i64; 0], tail: i64 }
struct Wide { xs: [i64; 64] }
struct LeftOwner { xs: TwoWords }
struct RightOwner { xs: ThreeWords }

fn read_struct(s: S) -> i64 {
    let a = s.xs
    return a[2] + s.tail
}

fn replace(s: S) -> S {
    s.xs = [1, 2, 9, 4]
    return s
}

fn index_mut(s: S) -> i64 {
    s.xs[1] = 77
    return s.xs[1] + s.tail
}

fn alias_field(s: AliasS) -> i64 { return s.xs[3] }

fn nested_index_mut(o: Outer) -> i64 {
    o.inner.xs[1] = 9
    return o.inner.xs[1]
}

fn branch_index_mut(s: S, flag: i64) -> i64 {
    if flag == 1 { s.xs[1] = 9 }
    return s.xs[1]
}

fn loop_index_mut(s: S) -> i64 {
    let mut i: i64 = 0
    while i < 2 { s.xs[i] = i + 5; i = i + 1 }
    return s.xs[1]
}

fn take(values: [i64; 2]) -> i64 { return values[1] }
fn make() -> [i64; 2] { return [8, 9] }
fn alias_take(values: Words) -> Elem { return values[1] }

fn float_read(s: F) -> f64 { return s.xs[1] }
fn one_read(s: One) -> i64 { return s.xs[0] + s.tail }
fn empty_read(s: Empty) -> i64 { return s.tail }
fn indexed_param4(s: S, i: i64) -> i64 { return s.xs[i] }
fn indexed_param64(s: Wide, i: i64) -> i64 { return s.xs[i] }

pub fn run() -> i64 {
    let s = S { xs: [10, 20, 30, 40], tail: 7 }
    let s = replace(s)
    let t = S { xs: [10, 20, 30, 40], tail: 7 }
    let a = AliasS { xs: [1, 2, 3, 6] }
    let o = Outer { inner: Inner { xs: [3, 4] } }
    return read_struct(s) + take(make()) + alias_take([11, 12]) + index_mut(t) + alias_field(a) + nested_index_mut(o) + branch_index_mut(t, 1) + loop_index_mut(t) + one_read(One { xs: [5], tail: 7 }) + empty_read(Empty { xs: [], tail: 13 })
}

pub fn float_run() -> f64 {
    let s = F { xs: [1.25, 2.5] }
    return float_read(s)
}

pub fn float_signed_zero() -> f64 {
    let s = F { xs: [-0.0, -3.25] }
    return s.xs[0]
}

pub fn indexed_read4() -> i64 {
    let s = S { xs: [10, 20, 30, 40], tail: 7 }
    return s.xs[2]
}

pub fn indexed_read64() -> i64 {
    let s = Wide { xs: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63] }
    return s.xs[37]
}

pub fn same_owner_left() -> i64 {
    let s = LeftOwner { xs: [11, 22] }
    return s.xs[1]
}

pub fn same_owner_right() -> i64 {
    let s = RightOwner { xs: [31, 32, 43] }
    return s.xs[2]
}
"#;

#[test]
fn fixed_array_struct_fields_run_native_artifact() {
    let mindc = common::mindc_bin();
    if !mindc.exists() {
        common::gate::skipped(
            "fixed_array_struct_field_run",
            "fixed-array-struct-field-run: mindc not found; skipping",
        );
        return;
    }
    let dir = tempfile::tempdir().expect("fixed-array scratch");
    let source = dir.path().join("fixed_array_struct_field.mind");
    let shared = dir.path().join("fixed_array_struct_field.so");
    std::fs::write(&source, SOURCE).expect("write fixed-array source");
    let out = Command::new(&mindc)
        .arg(&source)
        .arg("--emit-shared")
        .arg(&shared)
        .output()
        .expect("run mindc");
    assert!(
        out.status.success() && shared.is_file(),
        "fixed-array struct-field compile failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    unsafe {
        let lib = Library::new(&shared).expect("load fixed-array shared library");
        let run: Symbol<unsafe extern "C" fn() -> i64> = lib.get(b"run").expect("load run");
        let float_run: Symbol<unsafe extern "C" fn() -> f64> =
            lib.get(b"float_run").expect("load float_run");
        let float_signed_zero: Symbol<unsafe extern "C" fn() -> f64> = lib
            .get(b"float_signed_zero")
            .expect("load float_signed_zero");
        let indexed_read4: Symbol<unsafe extern "C" fn() -> i64> =
            lib.get(b"indexed_read4").expect("load indexed_read4");
        let indexed_read64: Symbol<unsafe extern "C" fn() -> i64> =
            lib.get(b"indexed_read64").expect("load indexed_read64");
        let same_owner_left: Symbol<unsafe extern "C" fn() -> i64> =
            lib.get(b"same_owner_left").expect("load same_owner_left");
        let same_owner_right: Symbol<unsafe extern "C" fn() -> i64> =
            lib.get(b"same_owner_right").expect("load same_owner_right");
        assert_eq!(run(), 16 + 9 + 12 + 84 + 6 + 9 + 9 + 6 + 12 + 13);
        assert_eq!(float_run().to_bits(), 2.5f64.to_bits());
        assert_eq!(float_signed_zero().to_bits(), (-0.0f64).to_bits());
        assert_eq!(indexed_read4(), 30);
        assert_eq!(indexed_read64(), 37);
        assert_eq!(same_owner_left(), 22);
        assert_eq!(same_owner_right(), 43);
    }
}

#[test]
fn indexed_struct_field_read_does_not_expand_with_field_length() {
    let module = parser::parse(SOURCE).expect("fixed-array struct source must parse");
    let ir = lower_to_ir(&module).expect("lowering");
    let body = |name: &str| {
        ir.instrs
            .iter()
            .find_map(|instr| match instr {
                Instr::FnDef {
                    name: fn_name,
                    body,
                    ..
                } if fn_name == name => Some(body.as_slice()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing lowered function `{name}`"))
    };
    let small = body("indexed_param4");
    let wide = body("indexed_param64");
    for (name, instructions) in [("small", small), ("wide", wide)] {
        assert!(
            instructions.iter().all(|instr| {
                !matches!(
                    instr,
                    Instr::ArrayLoad { .. }
                        | Instr::ArrayStore { .. }
                        | Instr::ConstArray { .. }
                        | Instr::ConstDenseTensor { .. }
                )
            }),
            "{name} indexed field read unexpectedly materialized an aggregate: {instructions:?}"
        );
        assert!(
            instructions.iter().any(|instr| {
                matches!(instr, Instr::Call { name, .. } if name == "__mind_oob_check")
            }),
            "{name} indexed field read omitted its bounds check: {instructions:?}"
        );
    }
    assert_eq!(small.len(), wide.len(), "indexed read IR expanded with N");
}

#[test]
fn unsupported_struct_array_cells_are_refused_before_lowering() {
    let mindc = common::mindc_bin();
    if !mindc.exists() {
        common::gate::skipped(
            "fixed_array_struct_field_unsupported",
            "fixed-array-struct-field-unsupported: mindc not found; skipping",
        );
        return;
    }
    let dir = tempfile::tempdir().expect("unsupported fixed-array scratch");
    let source = dir.path().join("unsupported_fixed_array_struct_field.mind");
    let shared = dir.path().join("unsupported_fixed_array_struct_field.so");
    std::fs::write(
        &source,
        "type U64Alias = u64\nstruct I {\n    xs: [i32; 2],\n}\nstruct F {\n    xs: [f32; 2],\n}\nstruct B {\n    xs: [bool; 2],\n}\nstruct U {\n    xs: [u64; 2],\n}\nstruct UA {\n    xs: [U64Alias; 2],\n}\nfn main() -> i64 {\n    let _i = I { xs: [1, 2] };\n    let _f = F { xs: [1.0, 2.0] };\n    let _b = B { xs: [1, 0] };\n    let _u = U { xs: [1, 2] };\n    let _ua = UA { xs: [1, 2] };\n    return 0;\n}\n",
    )
    .expect("write unsupported fixed-array source");
    let check = Command::new(&mindc)
        .arg("check")
        .arg(&source)
        .output()
        .expect("run mindc check");
    assert!(
        check.status.success(),
        "valid fixed-array types must pass check:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    let ir = Command::new(&mindc)
        .arg("--emit-ir")
        .arg(&source)
        .output()
        .expect("run mindc inspection");
    assert!(
        ir.status.success(),
        "valid fixed-array types must pass IR inspection:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&ir.stdout),
        String::from_utf8_lossy(&ir.stderr)
    );
    let build = Command::new(&mindc)
        .arg("--emit-shared")
        .arg(&shared)
        .arg(&source)
        .output()
        .expect("run mindc build");
    assert!(
        !build.status.success(),
        "unsupported fixed-array build unexpectedly passed"
    );
    let build_err = String::from_utf8_lossy(&build.stderr);
    assert!(
        build_err.contains("lower::fixed_struct_array_cell"),
        "build did not preserve structured runnable-capability refusal: {build_err}"
    );
    assert!(
        !build_err.contains("panicked at"),
        "unsupported fixed-array build reached a panic: {build_err}"
    );
}

#[test]
fn fixed_array_capability_gate_is_operation_scoped_and_project_aware() {
    let mindc = common::mindc_bin();
    if !mindc.exists() {
        common::gate::skipped(
            "fixed_array_struct_field_boundary",
            "fixed-array-struct-field-boundary: mindc not found; skipping",
        );
        return;
    }
    let root = common::scratch_dir("fixed_array_struct_field_boundary");
    let env_std = format!("{}/std", env!("CARGO_MANIFEST_DIR"));
    let run = |args: &[&str], cwd: Option<&std::path::Path>| {
        let mut command = Command::new(&mindc);
        command.args(args).env("MINDC_STD_DIR", &env_std);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        command.output().expect("run mindc boundary control")
    };
    let text = |out: &std::process::Output| {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };

    // A declaration without a construction/access operation has no backend
    // work and retains base125 runnable behavior.
    let unused = root.join("unused.mind");
    let unused_so = root.join("unused.so");
    std::fs::write(
        &unused,
        "struct Unused { xs: [u8; 2] }\nfn main() -> i64 { return 0 }\n",
    )
    .expect("write unused declaration control");
    let out = run(
        &[
            "--emit-shared",
            unused_so.to_str().unwrap(),
            unused.to_str().unwrap(),
        ],
        None,
    );
    assert!(out.status.success(), "unused declaration: {}", text(&out));
    assert!(
        unused_so.is_file(),
        "unused declaration emitted no artifact"
    );

    // Field-index mutation used to reach the generic fixed-array panic before
    // the post-lowering blocker could run. It must now refuse structurally.
    let mutation = root.join("mutation.mind");
    let mutation_so = root.join("mutation.so");
    std::fs::write(
        &mutation,
        "struct S { xs: [u8; 2] }\nfn main(s: S) -> i64 { s.xs[1] = 3; return 0 }\n",
    )
    .expect("write mutation control");
    let out = run(
        &[
            "--emit-shared",
            mutation_so.to_str().unwrap(),
            mutation.to_str().unwrap(),
        ],
        None,
    );
    let mutation_text = text(&out);
    assert_eq!(out.status.code(), Some(1), "mutation: {mutation_text}");
    assert!(
        mutation_text.contains("lower::fixed_struct_array_cell")
            && !mutation_text.contains("panicked at"),
        "mutation did not fail structurally: {mutation_text}"
    );
    assert!(
        !mutation_so.exists(),
        "mutation refusal emitted an artifact"
    );

    #[cfg(feature = "cross-module-imports")]
    {
        // Imported schemas use the defining module's qualified owner. The
        // unsupported `a.Bad` operation must block both project emitters while
        // an unused imported declaration remains runnable.
        for (tag, main_src, expect_success, owner) in [
            (
                "used",
                "import a;\nimport b;\npub fn run(p: a.Bad) -> i64 { return p.xs[1] }\n",
                false,
                Some("a.Bad.xs"),
            ),
            (
                "unused",
                "import a;\nimport b;\npub fn run() -> i64 { return 0 }\n",
                true,
                None,
            ),
        ] {
            let project = root.join(format!("project_{tag}"));
            std::fs::create_dir_all(project.join("src")).expect("create boundary project");
            std::fs::write(
                project.join("Mind.toml"),
                "[package]\nname=\"fixed_array_boundary\"\nversion=\"0.1.0\"\n\n[build]\nemit=\"cdylib\"\n",
            )
            .expect("write boundary manifest");
            std::fs::write(
                project.join("src/a.mind"),
                "pub struct Bad { xs: [u8; 2] }\n",
            )
            .expect("write unsupported owner");
            std::fs::write(
                project.join("src/b.mind"),
                "pub struct Bad { xs: [i64; 2] }\n",
            )
            .expect("write supported colliding owner");
            std::fs::write(project.join("src/main.mind"), main_src).expect("write boundary entry");
            for kind in ["object", "cdylib"] {
                let artifact = project.join(format!("{tag}-{kind}.out"));
                // Object projects derive the final object suffix from the
                // requested stem; cdylib honors --out verbatim. Intermediates
                // under target/obj are not final-artifact evidence.
                let expected_artifact = if kind == "object" {
                    artifact.with_extension("o")
                } else {
                    let file_name = artifact.file_name().expect("boundary artifact name");
                    artifact
                        .parent()
                        .expect("boundary artifact parent")
                        .join(format!("lib{}.so", file_name.to_string_lossy()))
                };
                let out = run(
                    &[
                        "build",
                        "--emit",
                        kind,
                        "--no-cache",
                        "--out",
                        artifact.to_str().unwrap(),
                    ],
                    Some(&project),
                );
                let rendered = text(&out);
                assert_eq!(
                    out.status.success(),
                    expect_success,
                    "project {tag}/{kind}: {rendered}"
                );
                if let Some(owner) = owner {
                    assert!(rendered.contains(owner), "project owner lost: {rendered}");
                    assert!(
                        !rendered.contains("panicked at"),
                        "project panicked: {rendered}"
                    );
                    assert!(
                        !artifact.exists() && !expected_artifact.exists(),
                        "project refusal emitted an artifact"
                    );
                } else {
                    assert!(
                        expected_artifact.is_file(),
                        "project unused emitted no final artifact: {}",
                        expected_artifact.display()
                    );
                }
            }
        }
    }
}
