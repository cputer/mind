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

//! Roadmap C6 — the opt-in native `mic@3 → mic@3` optimizer is WIRED into the
//! artifact-emitting path, and is byte-identical when off.
//!
//! Why this file exists. `optimize_mic3` had exactly one caller
//! (`ir::prepare_ir_for_backend`), whose only production caller was
//! `project::try_emit_mic` — the *failed-compile* embedded-JIT fallback. The
//! successful-compile pipeline inlined its own copy of prepare-for-backend
//! (`verify → canonicalize → verify`) that OMITTED the optimizer call, so the
//! pass was unreachable from every artifact path: `MIND_NATIVE_OPT=basic`
//! changed zero bytes of `--emit-mic3`. Nothing in `tests/`, `examples/` or
//! `scripts/` ever set `MIND_NATIVE_OPT`, so no gate caught it — the recorded
//! "keystone 7/7" was vacuously green, because with the pass unreachable the
//! keystone is green for BOTH env states.
//!
//! The two halves pinned here are the two halves of the C6 contract, and a
//! gate that checks only one of them is what let the regression hide:
//!
//!   1. `basic_changes_emitted_mic3_bytes` — turning the optimizer ON changes
//!      the emitted artifact. This is the WIRING proof; it fails closed if the
//!      pipeline ever again grows a private copy of backend-prep.
//!   2. `off_is_byte_identical_and_unknown_values_never_enable` — the default
//!      (and any unrecognised value) is a strict no-op, so keystone /
//!      cross-substrate canaries / frozen self-host seeds are untouched.
//!
//! Gated: `cargo test --release --features "mlir-build std-surface
//!         cross-module-imports" --test native_opt_wiring`.
//!
//! `std-surface` gate: the `Basic` pass schedule is itself `#[cfg(feature =
//! "std-surface")]` (it rewrites `If`/`While` regions and bitwise `BinOp`s that
//! do not exist in the minimal build), where `Basic` is a correct no-op. Test 1
//! would therefore be false-red without the feature, so the file is gated —
//! run it with the feature set above and confirm a POSITIVE test count, never
//! a bare exit 0.

#![cfg(feature = "std-surface")]

use libmind::ir::compact::emit_mic3;
use libmind::pipeline::{CompileOptions, compile_source_with_name};

/// Probe hitting three distinct `Basic` passes. `x` is a PARAM, so
/// `ir_canonical`'s constant folding — which runs in BOTH env states — cannot
/// touch any of these; only the native optimizer can. Keeping the operands
/// non-constant is what makes this a test of the optimizer rather than of
/// canonicalization.
///
///   * `(x + 1)` twice          -> `cse_binops`
///   * `(x * 0)`                -> `fold_mul_by_zero`
///   * `(x * 1)` and `(x + 0)`  -> `fold_algebraic_identities`
const PROBE: &str = "pub fn probe(x: i64) -> i64 {\n\
                     \x20   ((x + 1) + (x + 1)) + (((x * 0) + (x * 1)) + (x + 0))\n\
                     }\n";

/// Serialises the process-wide `MIND_NATIVE_OPT` mutation. Integration tests in
/// one binary run on parallel threads, so without this the two tests below
/// would race on the same variable and flake. `into_inner` on a poisoned lock:
/// a panicking test must not cascade into a spurious failure in the other.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Compiles `PROBE` with `MIND_NATIVE_OPT` set to `value` (or removed for
/// `None`) and returns the emitted mic@3 bytes — the exact bytes `mindc
/// <src> --emit-mic3 <out>` writes, which is `emit_mic3(&products.ir)` over the
/// same `CompileProducts` (pinned by `tests/mic3_cli_emit.rs`).
///
/// Rust 2024 makes `set_var`/`remove_var` unsafe because they are not
/// thread-safe; every call here is under `ENV_LOCK`, and the variable is
/// cleared before the guard drops so no state leaks between tests.
fn emit_with_opt(value: Option<&str>) -> Vec<u8> {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        match value {
            Some(v) => std::env::set_var("MIND_NATIVE_OPT", v),
            None => std::env::remove_var("MIND_NATIVE_OPT"),
        }
    }
    let products = compile_source_with_name(PROBE, Some("probe.mind"), &CompileOptions::default())
        .expect("probe must compile");
    let bytes = emit_mic3(&products.ir);
    unsafe {
        std::env::remove_var("MIND_NATIVE_OPT");
    }
    bytes
}

/// WIRING PROOF. `MIND_NATIVE_OPT=basic` must change the emitted artifact.
///
/// The assertion is deliberately "bytes differ AND the optimized artifact is
/// strictly smaller": mere inequality would also be satisfied by a pass that
/// perturbs the encoding, whereas a shorter artifact is positive evidence that
/// redundant `BinOp`s were actually eliminated.
#[test]
fn basic_changes_emitted_mic3_bytes() {
    let off = emit_with_opt(None);
    let basic = emit_with_opt(Some("basic"));

    assert_ne!(
        off, basic,
        "MIND_NATIVE_OPT=basic changed ZERO bytes of the emitted mic@3 — the C6 \
         optimizer is not reachable from the artifact-emitting path. Check that \
         pipeline::compile_source_with_name still calls ir::prepare_ir_for_backend \
         instead of inlining verify+canonicalize on its own."
    );
    assert!(
        basic.len() < off.len(),
        "optimized artifact ({} bytes) must be SMALLER than the unoptimized one \
         ({} bytes) — the probe contains a CSE-able duplicate and three algebraic \
         identities, so folding them must shrink the instruction stream",
        basic.len(),
        off.len()
    );
}

/// BYTE-IDENTITY PROOF. Default OFF is a strict no-op, and no unrecognised
/// value can silently enable a byte-changing pass.
///
/// This is the half that protects the keystone, the cross-substrate canaries
/// and the frozen self-host seeds: enabling C6 is a whole-corpus reseed event,
/// so it must never happen by accident (a typo'd value, a stray empty string in
/// CI). `opt_level_from_env` maps everything except the exact string `basic` to
/// `OptLevel::Off`; this pins that at the artifact surface rather than trusting
/// the mapping in isolation.
#[test]
fn off_is_byte_identical_and_unknown_values_never_enable() {
    let unset = emit_with_opt(None);

    for value in [
        "",
        "off",
        "OFF",
        "Basic",
        "BASIC",
        "1",
        "true",
        "aggressive",
    ] {
        assert_eq!(
            unset,
            emit_with_opt(Some(value)),
            "MIND_NATIVE_OPT={value:?} must be byte-identical to unset — only the \
             exact value \"basic\" may change emitted bytes"
        );
    }
}

/// DETERMINISM. The pass schedule is pinned (fixed order, bounded iteration —
/// never "loop until quiet"), so the optimized artifact that `trace_hash`
/// anchors and the canaries pin must be a deterministic function of the input.
#[test]
fn basic_is_deterministic_across_runs() {
    let first = emit_with_opt(Some("basic"));
    for _ in 0..4 {
        assert_eq!(
            first,
            emit_with_opt(Some("basic")),
            "optimized mic@3 must be identical run-to-run — a pass whose schedule \
             depends on hashmap iteration order would fail here"
        );
    }
}
