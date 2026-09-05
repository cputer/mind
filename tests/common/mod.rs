// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Shared test-harness helpers.
//!
//! # Staleness elimination (issue #42)
//!
//! The historic per-file `mindc_bin()` helper resolved the mindc binary by
//! preferring `target/debug/mindc` over `target/release/mindc`. A stale
//! `target/debug/mindc` — left over from an unrelated `cargo test` run that
//! did NOT compile with `--features mlir-build` — caused integration tests to
//! shell out to a WRONG-VERSION binary, producing pre-fix behaviour, parse
//! errors, or phantom bugs (false positives). Each such false positive
//! triggered a debugging chase against a problem that no longer existed.
//!
//! The fix: use `env!("CARGO_BIN_EXE_mindc")`. Cargo sets this environment
//! variable at *compile time* of the test binary itself, pointing at the
//! `mindc` binary that was built for the SAME invocation, SAME profile, and
//! SAME feature set. There is no staleness possible: the path is baked in
//! during the test binary's own compilation, and `cargo test` always rebuilds
//! `mindc` before running tests if any source changed.
//!
//! Reference: <https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates>
//! The `[[bin]] name = "mindc"` entry in `Cargo.toml` guarantees
//! `CARGO_BIN_EXE_mindc` is available to every integration test in this crate.

pub mod gate;

use std::path::PathBuf;

/// Return the path to the `mindc` binary for the CURRENT test-profile and
/// feature set.
///
/// Uses `env!("CARGO_BIN_EXE_mindc")` which is set by cargo at test-binary
/// compile time to the binary built alongside this test run — always correct,
/// never stale. No filesystem probing is required or performed.
///
/// Callers that tolerate a missing binary (skip semantics) should pair this
/// with an `.exists()` check; callers that require the binary should
/// `assert!(mindc_bin().exists(), "…build instructions…")`.
#[allow(dead_code)]
pub fn mindc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mindc"))
}

/// A scratch directory private to ONE test target in ONE process.
///
/// The std smokes wrote their `.so` to a FIXED shared path
/// (`std::env::temp_dir().join("mind_<x>_smoke.so")`). On a machine running two
/// test processes at once — two agents, two CI jobs on one runner, a `cargo
/// test` beside a preflight — one process's `mindc` truncates the artifact
/// another has just `dlopen`ed, and the flake reads as a compiler regression.
/// The path is also world-writable and predictable, so another user can
/// pre-create it as a symlink and redirect the write.
///
/// The directory carries the target name and the pid; the FILE names inside are
/// left alone so an artifact's identity is unchanged.
///
/// deferred: the std smokes named in the finding are routed; the remaining
/// `std::env::temp_dir()` sites across `tests/*.rs` still write to the shared
/// root. Upgrade path: route each through this helper as its file is next
/// touched — one helper, no per-file policy.
#[allow(dead_code)]
pub fn scratch_dir(target: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mind-{target}-{}", std::process::id()));
    std::fs::create_dir_all(&d)
        .unwrap_or_else(|e| panic!("create scratch dir {}: {e}", d.display()));
    d
}

/// Return the path to the `mind` CLI binary for the CURRENT test-profile and
/// feature set.
///
/// Same staleness argument as [`mindc_bin`], and the same defect it was written
/// for. Nine CLI/exec harnesses each hand-rolled
/// `CARGO_MANIFEST_DIR/target/{debug,release}/mind`, which is wrong twice over:
/// it ignores `CARGO_TARGET_DIR` (so the probe misses entirely whenever the
/// build is directed elsewhere) and it names a profile directory rather than the
/// binary cargo actually built for this run. The miss then landed on a
/// `println!("skipping"); return;`, so nine CLI gates asserted nothing and
/// reported green. `CARGO_BIN_EXE_mind` is baked in at the test binary's own
/// compile time by the `[[bin]] name = "mind"` entry and cannot be stale.
#[allow(dead_code)]
pub fn mind_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mind"))
}

/// Return `Some(path)` if the mindc binary exists, or `None` (suitable for
/// soft-skip inside a `#[test]`) if it does not.
///
/// A missing binary is only expected when the test is invoked without having
/// built mindc first (e.g. `cargo test --no-run` followed by manual deletion).
/// Under normal `cargo test` usage `CARGO_BIN_EXE_mindc` always points at a
/// freshly built binary.
#[allow(dead_code)]
pub fn mindc_or_skip() -> Option<PathBuf> {
    let p = mindc_bin();
    if p.exists() { Some(p) } else { None }
}
