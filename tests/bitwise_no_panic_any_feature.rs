// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the “License”);
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an “AS IS” BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Part of the MIND project (Machine Intelligence Native Design).

//! Bitwise source must never PANIC the compiler, in any feature configuration.
//!
//! Found by the #72 cross-substrate fuzzer (seed 0xDEADBEEF), which recorded
//! `lower_expr: no IR lowering for `Bitwise` in value position` — exit 101 —
//! and whose reproducer then sat untracked in the working tree.
//!
//! The defect was a subset-boundary disagreement between two layers. Bitwise is
//! a `std-surface` construct end to end: `ir::BinOp`'s BitAnd/BitOr/BitXor/Shl/
//! Shr variants, the `lower_expr` arm and the MLIR emitter are ALL gated on that
//! feature. The parser's operator table was not, so a `--no-default-features`
//! build (a live CI job, documented in Cargo.toml as "the low-level-only
//! subset") parsed `a | 1` into a `Node::Bitwise` that nothing downstream could
//! represent. It reached the fail-closed guard in `lower_expr` and panicked.
//!
//! DELIBERATELY NOT `required-features`. The bug existed only in the
//! configuration a std-surface-gated test cannot observe, so this file must
//! compile and assert in BOTH — that is the whole point of it. The two
//! configurations assert different things about the same invariant:
//!
//!   * with    `std-surface`: bitwise COMPILES (exit 0).
//!   * without `std-surface`: bitwise is REFUSED with a parse diagnostic that
//!     names the operator and carries a source offset (exit 1) — never a panic.
//!
//! Both halves share the non-negotiable assertion: exit code is never 101 and
//! stderr never contains "panicked at".

use std::process::Command;

/// The minimal form of the fuzzer's finding.
const BITWISE_VALUE_POSITION: &str = "pub fn g(a: i64) -> i64 {\n    return (a | 1);\n}\n";

/// Every operator in `BitOp`, so a future arm cannot regress just one of them.
const ALL_BITWISE_OPS: &str = "pub fn g(a: i64) -> i64 {\n\
     \x20   return (((((a | 1) & 3) ^ 5) << 1) >> 2);\n\
     }\n";

/// The compound `op=` desugar, which builds `Node::Bitwise` at a second site.
const BITWISE_COMPOUND: &str =
    "pub fn h(a: i64) -> i64 {\n    let mut x: i64 = a;\n    x |= 3;\n    return x;\n}\n";

/// The verbatim program from the #72 fuzzer reproducer (seed 0xDEADBEEF).
const FUZZ_REPRODUCER: &str = "pub fn f(a: i64) -> i64 {\n\
     \x20   let v0: i64 = (a / (((a >> 1) << 3) | 1));\n\
     \x20   let v1: i64 = (a << 3);\n\
     \x20   let v2: i64 = if a <= v0 { ((a / (v0 | 1)) / ((v1 / (v1 | 1)) | 1)) } else { 8 };\n\
     \x20   let v3: i64 = 8;\n\
     \x20   return (v3 + v0);\n\
     }\n";

/// Monotonic per-invocation nonce. `tag` alone is NOT unique: the shared-
/// invariant tests and the configuration-specific test compile the same
/// fixtures under the same tags, libtest runs them on parallel threads, and
/// two calls that agreed on a directory name would race — one removing the
/// tree while the other is still compiling in it.
static COMPILE_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Compile `src` with the freshly-built `mindc`, returning (exit code, stderr).
fn compile(src: &str, tag: &str) -> (Option<i32>, String) {
    let seq = COMPILE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mind_bitwise_gate_{tag}_{}_{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let src_path = dir.join("in.mind");
    let out_path = dir.join("out.mic3");
    std::fs::write(&src_path, src).expect("write source");

    let r = Command::new(env!("CARGO_BIN_EXE_mindc"))
        .args([
            src_path.to_str().unwrap(),
            "--emit-mic3",
            out_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn mindc");

    let _ = std::fs::remove_dir_all(&dir);
    (
        r.status.code(),
        String::from_utf8_lossy(&r.stderr).into_owned(),
    )
}

/// The invariant that holds in EVERY configuration: refusal is fine, a panic is
/// not. 101 is the rustc panic exit code.
fn assert_never_panics(src: &str, tag: &str) -> (Option<i32>, String) {
    let (code, stderr) = compile(src, tag);
    assert!(
        !stderr.contains("panicked at"),
        "{tag}: compiler PANICKED on bitwise source.\n--- stderr ---\n{stderr}"
    );
    assert_ne!(
        code,
        Some(101),
        "{tag}: compiler exited 101 (panic) on bitwise source.\n--- stderr ---\n{stderr}"
    );
    (code, stderr)
}

#[test]
fn bitwise_value_position_never_panics() {
    assert_never_panics(BITWISE_VALUE_POSITION, "value_position");
}

#[test]
fn every_bitwise_operator_never_panics() {
    assert_never_panics(ALL_BITWISE_OPS, "all_ops");
}

#[test]
fn bitwise_compound_assign_never_panics() {
    assert_never_panics(BITWISE_COMPOUND, "compound");
}

#[test]
fn fuzz_seed_deadbeef_reproducer_never_panics() {
    assert_never_panics(FUZZ_REPRODUCER, "fuzz_deadbeef");
}

// ---------------------------------------------------------------------------
// Configuration-specific halves.
// ---------------------------------------------------------------------------

/// With `std-surface`, bitwise is a supported construct and must COMPILE.
#[cfg(feature = "std-surface")]
#[test]
fn bitwise_compiles_under_std_surface() {
    for (src, tag) in [
        (BITWISE_VALUE_POSITION, "value_position"),
        (ALL_BITWISE_OPS, "all_ops"),
        (BITWISE_COMPOUND, "compound"),
        (FUZZ_REPRODUCER, "fuzz_deadbeef"),
    ] {
        let (code, stderr) = assert_never_panics(src, tag);
        assert_eq!(code, Some(0), "{tag}: expected a clean compile.\n{stderr}");
    }
}

/// Without `std-surface` the construct is outside the documented subset, so it
/// must be REFUSED — with a diagnostic that names the operator and points at a
/// source offset, rather than the fail-closed panic this test was written for.
#[cfg(not(feature = "std-surface"))]
#[test]
fn bitwise_refused_with_a_diagnostic_without_std_surface() {
    for (src, tag, op) in [
        (BITWISE_VALUE_POSITION, "value_position", "`|`"),
        (ALL_BITWISE_OPS, "all_ops", "`|`"),
        (BITWISE_COMPOUND, "compound", "`|=`"),
        (FUZZ_REPRODUCER, "fuzz_deadbeef", "`>>`"),
    ] {
        let (code, stderr) = assert_never_panics(src, tag);
        assert_eq!(code, Some(1), "{tag}: expected a refusal.\n{stderr}");
        assert!(
            stderr.contains("std-surface"),
            "{tag}: diagnostic must name the missing feature.\n{stderr}"
        );
        assert!(
            stderr.contains(op),
            "{tag}: diagnostic must name the operator {op}.\n{stderr}"
        );
        assert!(
            stderr.contains(".mind:"),
            "{tag}: diagnostic must carry a source location.\n{stderr}"
        );
    }
}
