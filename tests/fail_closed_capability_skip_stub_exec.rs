// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

#![cfg(unix)]

//! The call-site wrapper of the fail-CLOSED capability-skip contract, driven by
//! a REAL child process.
//!
//! `tests/fail_closed_capability_skip.rs` owns the classifier's contract over
//! verbatim fixtures and is platform-independent. These four tests prove the
//! same contract through `gate::compiled_with`, i.e. against a
//! `std::process::Output` that an actual spawn produced — the shape every one
//! of the 24 converted call sites uses. Manufacturing that `Output` by hand
//! would delete the only end-to-end evidence these tests carry, so a stub
//! compiler is spawned for real.
//!
//! # Why this file is unix-only AT FILE SCOPE
//!
//! The stub is a POSIX shell script. `ci.yml`'s `build_test` job is a 4-row
//! matrix (`ubuntu-latest`, `macos-latest`, `macos-14`, `windows-latest`) whose
//! Test step runs `cargo test --verbose --no-default-features` with no `if:`
//! guard, so an auto-discovered test file runs on Windows too. There
//! `Command::new(stub)` reaches `CreateProcessW`, which rejects a non-PE image
//! with `ERROR_BAD_EXE_FORMAT` (os error 193) — not the `ETXTBSY` the retry
//! loop below waits out — and the spawn panics. Four tests would fail
//! deterministically on a host with nothing wrong with it.
//!
//! The gate is at FILE scope, not on each item, for the same reason
//! `tests/array_oob_trap_run.rs` and `tests/array_store_run.rs` put theirs
//! there: an item-level `#[cfg(unix)]` is per-item, so the next test appended
//! to the file is un-gated by default. `tests/harness_portability.rs` enforces
//! that mechanically, and the platform-independent tests stay in the sibling
//! file so they keep running on all four rows.
//!
//! deferred: the four contracts below are unproven on `windows-latest`. Closing
//! the gap needs a Windows-native stub (a `.cmd` written and driven through
//! `cmd /C`, whose `echo`-to-stderr quoting cannot be verified on the Linux
//! development host) plus a matrix row that actually exercises it — owned by
//! the fail-closed capability-skip gate (`tests/common/gate.rs`). Until then
//! `gate::compiled_with` is proven end-to-end on unix only; its classifier,
//! which holds the whole decision, is proven on every row by the sibling file.

mod common;

use common::gate;
use std::path::{Path, PathBuf};

/// The stderr `mindc --emit-shared` prints when it was built without
/// `mlir-build` — a capability gap. Built from the compiler's OWN cause-code
/// constant rather than a second hand-copied fixture, so this file cannot drift
/// away from the wire shape the classifier reads.
fn cap_feature_stderr() -> String {
    format!(
        "error[build][{}]: --emit-shared requires building with the 'mlir-build' feature",
        libmind::diagnostics::capability::NO_NATIVE_BACKEND
    )
}

fn stub_compiler(name: &str, exit_code: i32, stderr: &str) -> PathBuf {
    // Per-PROCESS directory: a fixed shared path made two concurrent runs of
    // this harness write and exec the same file, and the loser saw ETXTBSY —
    // a flake in the very gate that exists to remove false results.
    let dir = std::env::temp_dir().join(format!("mind_fail_closed_stub_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir stub dir");
    let p = dir.join(name);
    std::fs::write(
        &p,
        format!("#!/bin/sh\nprintf '%s' \"{stderr}\" >&2\nexit {exit_code}\n"),
    )
    .expect("write stub");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    p
}

/// Execute a just-written stub, retrying through the write-then-exec race.
///
/// `ETXTBSY` is TRANSIENT here and has nothing to do with what is being tested:
/// this harness is multi-threaded, so between one thread writing a stub and
/// exec'ing it, another thread's `Command::spawn` can fork and inherit the
/// still-open write fd — the child holds it until its own `exec`, and the
/// kernel refuses to exec a file open for writing. Measured: adding two tests
/// that spawn the real `mindc` (a much longer fork→exec window under
/// `mlir-build`, which shells out to `mlir-opt`/`clang`) turned this into a
/// 1-in-3 flake that struck a DIFFERENT test each run. A flaky gate is a gate
/// nobody believes, so the transient condition is waited out rather than
/// reported as a result; every other spawn error still fails loudly.
fn run_stub(stub: &Path) -> std::process::Output {
    /// ~500 ms of retries: the window closes as soon as the racing child execs.
    const MAX_ATTEMPTS: u32 = 50;
    for _ in 0..MAX_ATTEMPTS {
        match std::process::Command::new(stub).output() {
            Ok(out) => return out,
            Err(e) if e.raw_os_error() == Some(libc_etxtbsy()) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => panic!("run stub compiler {}: {e}", stub.display()),
        }
    }
    panic!(
        "stub {} stayed ETXTBSY for {MAX_ATTEMPTS} attempts",
        stub.display()
    )
}

/// `ETXTBSY`. Hard-coded rather than pulled from a dependency: this harness has
/// none, and 26 is the value fixed by BOTH unix errno ABIs the CI matrix runs —
/// Linux and Darwin. The file-scope `#![cfg(unix)]` above is what makes that an
/// exhaustive list: no other ABI can reach this constant.
const fn libc_etxtbsy() -> i32 {
    26
}

#[test]
fn broken_compiler_panics_through_the_call_site_wrapper() {
    let stub = stub_compiler("mindc_broken", 1, "error: mismatched types");
    let out = run_stub(&stub);
    let r = std::panic::catch_unwind(|| gate::compiled_with("stub-target", &out, false));
    let msg = r.expect_err("a broken compiler must PANIC, not skip");
    let text = msg
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| String::from("<non-string panic>"));
    assert!(
        text.contains("stub-target") && text.contains("mismatched types"),
        "panic must name the target and quote stderr, got: {text}"
    );
}

#[test]
fn capability_stub_skips_through_the_call_site_wrapper() {
    let stub = stub_compiler("mindc_no_cap", 1, &cap_feature_stderr());
    let out = run_stub(&stub);
    assert!(
        !gate::compiled_with("stub-target", &out, false),
        "a genuine capability gap must still skip"
    );
}

#[test]
fn capability_stub_panics_under_enforcement() {
    let stub = stub_compiler("mindc_no_cap_req", 1, &cap_feature_stderr());
    let out = run_stub(&stub);
    let r = std::panic::catch_unwind(|| gate::compiled_with("stub-target", &out, true));
    assert!(
        r.is_err(),
        "MIND_BENCH_REQUIRE=1 must turn a capability skip into a hard failure"
    );
}

#[test]
fn working_compiler_reports_compiled() {
    let stub = stub_compiler("mindc_ok", 0, "");
    let out = run_stub(&stub);
    assert!(gate::compiled_with("stub-target", &out, false));
    assert!(gate::compiled_with("stub-target", &out, true));
}
