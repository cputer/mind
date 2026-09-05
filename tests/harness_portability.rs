// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Portability contract for the integration-test harness.
//!
//! # The defect this gate defends against
//!
//! `ci.yml`'s `build_test` job is a 4-row matrix — `ubuntu-latest`,
//! `macos-latest`, `macos-14` and `windows-latest` — with `fail-fast: false`,
//! and its Test step runs `cargo test --verbose --no-default-features` with no
//! `if:` guard. Every auto-discovered file under `tests/` is therefore BUILT
//! AND RUN on Windows unless it says otherwise.
//!
//! A test that writes an extensionless POSIX shell script and execs it cannot
//! work there: `Command::new(path)` reaches `CreateProcessW`, which rejects a
//! non-PE image with `ERROR_BAD_EXE_FORMAT` (os error 193). That is not the
//! `ETXTBSY` a write-then-exec retry loop tolerates, so the spawn panics and
//! the Windows row goes red deterministically — on a host that never had a
//! defect to report.
//!
//! # The rule
//!
//! A test file that writes a POSIX shell script must declare itself unix-only
//! AT FILE SCOPE (`#![cfg(...unix...)]`), so cargo does not build it on a host
//! that cannot run it. An item-level `#[cfg(unix)]` on each affected test is
//! NOT enough: it is per-item, so the next test appended to the file is
//! un-gated by default — which is exactly the shape that introduced this
//! defect. `tests/array_oob_trap_run.rs` and `tests/array_store_run.rs`
//! already carry the file-scope form; this gate is what makes that the rule
//! rather than a habit.
//!
//! This file is deliberately NOT gated: the scan is pure source reading, so it
//! runs — and the rule is enforced — on every row of the matrix.

use std::path::{Path, PathBuf};

/// A POSIX shell shebang, the payload of a shell stub.
///
/// Assembled from two pieces so the scanner cannot match its OWN source. An
/// exemption list would be a hole that erodes with every new entry; a marker
/// that is unable to name itself has no hole to erode.
const SHELL_STUB_MARKER: &str = concat!("#!", "/bin/");

/// Does this source write a POSIX shell script?
fn writes_a_shell_stub(text: &str) -> bool {
    text.contains(SHELL_STUB_MARKER)
}

/// Does this source refuse to be built on a non-unix host, at FILE scope?
///
/// Only an inner attribute (`#![cfg(...)]`) counts: it governs the whole file,
/// including tests that do not exist yet.
fn declares_unix_only(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("#![cfg(") && t.contains("unix")
    })
}

fn test_sources() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).expect("read tests dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read test source");
            (p, text)
        })
        .collect()
}

#[test]
fn every_shell_stub_test_is_unix_only_at_file_scope() {
    let mut ungated = Vec::new();
    for (path, text) in test_sources() {
        if writes_a_shell_stub(&text) && !declares_unix_only(&text) {
            ungated.push(path.display().to_string());
        }
    }
    assert!(
        ungated.is_empty(),
        "these test files write a POSIX shell script but are built on every row \
         of the 4-OS `build_test` matrix, including `windows-latest`, where \
         exec'ing one fails with ERROR_BAD_EXE_FORMAT (os error 193) and reds \
         the row. Add a file-scope `#![cfg(unix)]` (see \
         tests/array_oob_trap_run.rs), moving the platform-independent tests to \
         their own file rather than gating them too.\n  {}",
        ungated.join("\n  ")
    );
}

#[test]
fn the_portability_scan_can_still_see_the_bad_shape() {
    // Positive control: the scan is only evidence if it fails on the shape it
    // forbids, and passes on the shape it prescribes.
    let bad = format!("fn stub() {{\n    write(p, \"{SHELL_STUB_MARKER}sh\\nexit 1\\n\");\n}}\n");
    assert!(writes_a_shell_stub(&bad) && !declares_unix_only(&bad));

    let good = format!("#![cfg(unix)]\n{bad}");
    assert!(writes_a_shell_stub(&good) && declares_unix_only(&good));

    // An item-level guard is NOT the file-scope declaration this rule requires.
    let item_level = format!("#[cfg(unix)]\n{bad}");
    assert!(!declares_unix_only(&item_level));

    // The compound form the repo already uses must count.
    assert!(declares_unix_only(
        "#![cfg(all(unix, feature = \"mlir-build\", feature = \"std-surface\"))]\n"
    ));
}
