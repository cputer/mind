// Copyright 2025 STARGA Inc. Licensed under the Apache License, Version 2.0.
//
//! `mindc build` / `mindc run` must consult the runnable-artifact ABI gate (#54).
//!
//! `products.runnable_blockers` — the constructs the shipped i64-scalar backend
//! would SILENTLY MISCOMPILE — was consulted ONLY on the single-file
//! `--emit-obj` / `--emit-shared` path (`src/bin/mindc.rs`). The `mindc build` /
//! `mindc run` compile path (`build::run_build` -> `project::compile_sources` ->
//! `compile_single_source`, and the cdylib `build_cdylib_from_entry`) NEVER
//! checked it — so a program that `--emit-shared` fail-loud REJECTS built GREEN
//! (rc=0) and ran WRONG via the primary commands. The fix consults
//! `runnable_blockers` in the build/run compile path and fails non-zero.
//!
//! This test uses the enum-handle-in-scalar-return blocker (`divide(_,0)` returns
//! `Res::Err(0)` where `-> i64` is declared — a leaked heap-record pointer),
//! confirms `--emit-shared` rejects it, then asserts `mindc build` AND `mindc run`
//! now exit NON-ZERO (they returned rc=0 before).
//!
//! Gate: `cargo test --features "std-surface mlir-build cross-module-imports"
//!                   --test build_run_runnable_blocker_gate`

#![cfg(all(
    unix,
    feature = "mlir-build",
    feature = "std-surface",
    feature = "cross-module-imports"
))]

mod common;
use common::mindc_bin;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

// Bare-scalar `-> i64` return that yields an enum constructor handle on one path
// — a `runnable_blocker` (`lower::enum_handle_in_scalar_return`).
const BLOCKER_MIND: &str = r#"enum Res { Ok(i64), Err(i64) }
fn divide(a: i64, b: i64) -> i64 {
    if b == 0 { return Res::Err(0); }
    return a / b;
}
fn main() -> i64 { return divide(4, 2); }
"#;

fn manifest(entry: &str) -> String {
    format!(
        "[package]\nname = \"blockerproj\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"{entry}\"\noutput = \"blockerproj\"\n"
    )
}

fn linked_manifest(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n\
         [build]\nentry = \"src/main.mind\"\noutput = \"{name}\"\n\n\
         [targets.cpu]\nbackend = \"cpu\"\nsources = [\"src/main.mind\", \"src/helper.mind\"]\n"
    )
}

fn write_linked_project(root: &Path, name: &str, main_source: &str, helper_source: &str) {
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("mkdir project src");
    fs::write(src_dir.join("main.mind"), main_source).expect("write project entry");
    fs::write(src_dir.join("helper.mind"), helper_source).expect("write project helper");
    fs::write(root.join("Mind.toml"), linked_manifest(name)).expect("write project manifest");
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_project_fallback_refusal(output: &Output, operation: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "{operation} must refuse a non-entry fallback: {}",
        output_text(output)
    );
    assert!(
        stderr.contains("[E5005]"),
        "{operation} must preserve the source-fallback cause code: {}",
        output_text(output)
    );
    assert!(
        stderr.contains("helper") && stderr.contains("not natively compiled"),
        "{operation} must identify the non-entry fallback module: {}",
        output_text(output)
    );
}

#[test]
fn build_and_run_reject_runnable_blocker() {
    let mindc = mindc_bin();
    if !mindc.exists() {
        crate::common::gate::skipped(
            "build_run_runnable_blocker_gate",
            "build-run-runnable-blocker-gate: mindc not found; skipping",
        );
        return;
    }
    let td = tempfile::tempdir().expect("tempdir");
    let src_dir = td.path().join("src");
    fs::create_dir_all(&src_dir).expect("mkdir src");
    fs::write(src_dir.join("main.mind"), BLOCKER_MIND).expect("write src");
    fs::write(td.path().join("Mind.toml"), manifest("src/main.mind")).expect("write manifest");

    // (0) Baseline: the single-file `--emit-shared` path already rejects it.
    let so = td.path().join("blocker.so");
    let es = Command::new(&mindc)
        .args([
            src_dir.join("main.mind").to_str().unwrap(),
            "--emit-shared",
            so.to_str().unwrap(),
        ])
        .output()
        .expect("run mindc --emit-shared");
    let es_err = String::from_utf8_lossy(&es.stderr);
    if crate::common::gate::is_capability_gap(&es_err) {
        crate::common::gate::skipped(
            "build_run_runnable_blocker_gate",
            "build-run-runnable-blocker-gate: needs mlir-build; skipping",
        );
        return;
    }
    assert!(
        !es.status.success(),
        "--emit-shared must reject the runnable_blocker but exited 0"
    );
    assert!(
        es_err.contains("enum_handle_in_scalar_return"),
        "expected the enum-handle-scalar-return diagnostic; got:\n{es_err}"
    );

    // (1) `mindc build` must now ALSO reject it (was rc=0 before the fix).
    let build = Command::new(&mindc)
        .arg("build")
        .current_dir(td.path())
        .output()
        .expect("run mindc build");
    assert!(
        !build.status.success(),
        "mindc build must fail non-zero on a runnable_blocker; stdout={} stderr={}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr),
    );
    assert!(
        String::from_utf8_lossy(&build.stderr).contains("runnable artifact"),
        "mindc build error should name the runnable-artifact refusal; got:\n{}",
        String::from_utf8_lossy(&build.stderr),
    );

    // (2) `mindc run` must now ALSO reject it (was rc=0 before the fix).
    let run = Command::new(&mindc)
        .arg("run")
        .current_dir(td.path())
        .output()
        .expect("run mindc run");
    assert!(
        !run.status.success(),
        "mindc run must fail non-zero on a runnable_blocker; stdout={} stderr={}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
}

/// #244 project coverage: a warning-only, type-check-clean self-host intrinsic
/// in a LINKED non-entry source must not be turned into a green project build
/// or run. This exercises the source-fallback accounting in
/// `compile_sources`, rather than the entry-only single-file guard above.
#[test]
fn project_build_and_run_reject_non_entry_source_fallback() {
    let mindc = mindc_bin();
    if !mindc.exists() {
        crate::common::gate::skipped(
            "build_run_runnable_blocker_gate",
            "project-fallback-gate: mindc not found; skipping",
        );
        return;
    }

    let td = tempfile::tempdir().expect("tempdir");
    write_linked_project(
        td.path(),
        "project_fallback",
        "import helper;\nfn main() -> i64 {\n    return degraded();\n}\n",
        "pub fn degraded() -> i64 {\n    let h: i64 = __mind_nerve_route(1);\n    return h;\n}\n",
    );

    // The helper is intentionally check-clean: E2024 is an advisory warning
    // for an unregistered __mind_* intrinsic. The project build is the seam
    // under test; a check failure here would exercise the wrong contract.
    let check = Command::new(&mindc)
        .args(["check", "src/helper.mind"])
        .current_dir(td.path())
        .output()
        .expect("run project check");
    assert!(
        check.status.success(),
        "fallback fixture must pass check with an advisory diagnostic: {}",
        output_text(&check)
    );
    let check_text = output_text(&check);
    assert!(
        check_text.contains("E2024") && check_text.contains("__mind_nerve_route"),
        "check must expose the advisory unsupported intrinsic: {check_text}"
    );

    // `--out` is outside the project target directory so this assertion binds
    // the user-visible artifact path, not an intermediate object/cache.
    let requested = td.path().join("requested-fallback");
    let build = Command::new(&mindc)
        .args(["build", "--no-cache", "--out"])
        .arg(&requested)
        .current_dir(td.path())
        .output()
        .expect("run project build");
    let build_text = output_text(&build);
    assert_project_fallback_refusal(&build, "project build");
    assert!(
        build_text.contains("E2024") && build_text.contains("__mind_nerve_route"),
        "build refusal must retain the helper's E2024 cause, rather than accepting an arbitrary resolver failure: {build_text}"
    );
    assert!(
        !requested.exists(),
        "a refused non-entry fallback build must leave no requested runnable artifact at {}",
        requested.display()
    );

    // Remove any intermediate target output before `run`, so a positive
    // absence assertion cannot be satisfied by stale state from the build
    // invocation above.
    if td.path().join("target").exists() {
        fs::remove_dir_all(td.path().join("target")).expect("remove failed-build target");
    }
    let run = Command::new(&mindc)
        .arg("run")
        .current_dir(td.path())
        .output()
        .expect("run project run");
    let run_text = output_text(&run);
    assert_project_fallback_refusal(&run, "project run");
    assert!(
        run_text.contains("E2024") && run_text.contains("__mind_nerve_route"),
        "run refusal must retain the helper's E2024 cause, rather than accepting an arbitrary resolver failure: {run_text}"
    );
    assert!(
        !td.path().join("target/debug/project_fallback").exists(),
        "a refused non-entry fallback run must leave no fresh runnable artifact"
    );
}

/// A valid two-file project remains a real native build/run control for the
/// non-entry fallback gate. This prevents the refusal from becoming a blanket
/// rejection of linked dependencies.
#[test]
fn project_valid_linked_dependency_builds_and_runs() {
    let mindc = mindc_bin();
    if !mindc.exists() {
        crate::common::gate::skipped(
            "build_run_runnable_blocker_gate",
            "project-link-control: mindc not found; skipping",
        );
        return;
    }

    let td = tempfile::tempdir().expect("tempdir");
    write_linked_project(
        td.path(),
        "project_link_control",
        "import helper;\nfn main() -> i64 { return answer(); }\n",
        "pub fn answer() -> i64 { return 42; }\n",
    );
    let requested = td.path().join("linked-project");
    let build = Command::new(&mindc)
        .args(["build", "--no-cache", "--out"])
        .arg(&requested)
        .current_dir(td.path())
        .output()
        .expect("run valid project build");
    assert!(
        build.status.success(),
        "valid linked project build failed: {}",
        output_text(&build)
    );
    assert!(requested.is_file(), "valid build produced no artifact");

    let run = Command::new(&mindc)
        .arg("run")
        .current_dir(td.path())
        .output()
        .expect("run valid project run");
    assert_eq!(
        run.status.code(),
        Some(42),
        "valid linked project did not execute its dependency result: {}",
        output_text(&run)
    );
}
