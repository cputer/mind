// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// IR-preservation over the stdlib modules requires lowering std-surface
// constructs (struct/enum literals, bitwise ops), so the whole gate is scoped
// to `std-surface` — the feature the stdlib (and the self-host keystone) build
// under. Under `--no-default-features` these modules are out of lowering scope
// by design, so the binary compiles to zero tests there.
#![cfg(feature = "std-surface")]

//! Formatter IR-preservation gate — Phase 2A acceptance test (Step 3 of PR #3).
//!
//! For every file in scope, asserts that formatting never changes the
//! compiled canonical image:
//!
//!   `emit_mic3(compile(src)) == emit_mic3(compile(format_source(src)))`
//!
//! This is the semantic-correctness gate: a formatter that changes the
//! program's meaning (IR) is a compiler bug, not just a style issue.
//!
//! Scope: [`IN_SCOPE_FILES`] — the single source of truth shared by the
//! per-file tests and by `ir_preservation_summary`, so the two can never
//! drift apart.
//!
//! # Why mic@3 and not mic@1
//!
//! This gate used to compare `compile_to_mic_text` (= `ir::save`, the mic@1
//! *text* form), and was vacuous for its entire life as a result: the mic@1
//! instruction emitter has arms only for tensor/scalar ops and ends in a
//! catch-all that drops `FnDef`/`Call`/`Return`/`Param`/`While`/`If`/array
//! and region instructions. Every file in [`IN_SCOPE_FILES`] is entirely
//! functions, so all eight compiled to the same ~250 bytes of
//! `const.i64 0 / O N0` and no change inside any function body could move
//! the compared text. mic@1 is a debug/inspection form, **not** an
//! equivalence oracle.
//!
//! mic@3 is the canonical artifact: it carries the full body instruction
//! stream and is the same emitter behind `mindc --emit-mic3` and
//! `ir::ir_trace_hash`. [`ir_image_oracle_sees_function_body_change`] is the
//! permanent positive control that pins this property — it mutates one token
//! inside a function body and requires the two images to differ, so a future
//! re-point at a blind oracle fails here instead of going quietly vacuous.
//!
//! # The gate is fail-closed on skips
//!
//! A file whose *original* source does not compile is not an IR-preservation
//! failure, but a per-file test that silently passes because its file was
//! never compiled is a gate that cannot fail. So a skip is a FAILURE unless
//! the file carries an explicit [`SKIP_ALLOWLIST`] entry naming the reason,
//! and a missing file is always a failure. [`Checked`] is `#[must_use]`, so a
//! call site that drops the verdict does not compile. `ir_preservation_summary`
//! asserts the exercised/skipped counts against that allowlist rather than
//! merely printing them.
//!
//! # What "byte-identical canonical image" means
//!
//! [`ir_image`] parses → type-checks → lowers to IR → verifies →
//! canonicalizes → emits mic@3.  Two sources that lower to identical IR
//! produce byte-identical mic@3.  Formatting must not rename variables,
//! reorder top-level items, or alter the AST in any way that changes the
//! lowered result.

use libmind::fmt::format_source;
use libmind::ir::compact::emit_mic3;
use libmind::pipeline::{CompileError, CompileOptions, compile_source};
use libmind::project::MindcraftFormatConfig;

fn default_cfg() -> MindcraftFormatConfig {
    MindcraftFormatConfig::default()
}

fn default_compile_opts() -> CompileOptions {
    CompileOptions::default()
}

fn manifest_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ---------------------------------------------------------------------------
// Gate scope
// ---------------------------------------------------------------------------

/// Every file this gate covers, relative to the crate manifest dir.
///
/// Both the per-file tests and `ir_preservation_summary` read their scope
/// from here: a file added to the gate cannot be covered by one and missed
/// by the other.
const IN_SCOPE_FILES: &[&str] = &[
    "std/vec.mind",
    "std/string.mind",
    "std/io.mind",
    "std/map.mind",
    "std/blas.mind",
    "examples/parser/main.mind",
    "examples/typecheck/main.mind",
    "examples/emit_ir/main.mind",
];

/// Files permitted to be skipped because their *original* source is outside
/// the compile-to-MIC scope, each with the reason.
///
/// Empty: every in-scope file compiles under `std-surface` today. An entry
/// here narrows the gate, so it is a deliberate, reviewed decision — and it
/// is self-cleaning: once the named file compiles again, its stale entry
/// fails this test rather than quietly shrinking coverage forever.
const SKIP_ALLOWLIST: &[(&str, &str)] = &[];

fn allowed_skip(rel: &str) -> Option<&'static str> {
    SKIP_ALLOWLIST
        .iter()
        .find(|(name, _)| *name == rel)
        .map(|(_, reason)| *reason)
}

// ---------------------------------------------------------------------------
// Comparison oracle
// ---------------------------------------------------------------------------

/// Compile `src` to its canonical mic@3 image — the bytes this gate compares.
///
/// mic@3 carries the whole body instruction stream, so a change inside a
/// function body moves these bytes. Do **not** swap this back to
/// `compile_to_mic_text` (mic@1): that form drops function bodies entirely
/// and makes every assertion in this file vacuous — see the module docs and
/// [`ir_image_oracle_sees_function_body_change`].
fn ir_image(src: &str, opts: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    Ok(emit_mic3(&compile_source(src, opts)?.ir))
}

/// Summarise how two canonical images differ without dumping the whole
/// binary into a failure message.
fn describe_image_diff(before: &[u8], after: &[u8]) -> String {
    match before.iter().zip(after.iter()).position(|(a, b)| a != b) {
        Some(i) => format!(
            "len {} -> {}; first differing byte at offset {i}: 0x{:02x} -> 0x{:02x}",
            before.len(),
            after.len(),
            before[i],
            after[i]
        ),
        None => format!(
            "len {} -> {}; the shorter image is a prefix of the longer one",
            before.len(),
            after.len()
        ),
    }
}

// ---------------------------------------------------------------------------
// Core assertion helper
// ---------------------------------------------------------------------------

/// Verdict for one file: either the gate actually ran on it, or it did not.
///
/// `#[must_use]` on the type (not just on the function) is what keeps every
/// call site honest: dropping the verdict is exactly how this gate went
/// vacuous, and it now fails to compile rather than reading as a pass.
#[must_use]
#[derive(Debug)]
enum Checked {
    /// The original compiled and the before/after images matched.
    Exercised,
    /// The *original* source did not compile, so formatting was never checked
    /// against it. Carries the compile error so a fail-closed skip names its
    /// cause instead of hiding it.
    Skipped(String),
}

/// Assert IR-preservation for a single source string.
fn check_ir_preservation(label: &str, src: &str) -> Checked {
    let cfg = default_cfg();
    let opts = default_compile_opts();

    // Step 1: compile the original source.
    let ir_before = match ir_image(src, &opts) {
        Ok(ir) => ir,
        Err(e) => {
            // Source uses features outside the compile-to-IR scope
            // (e.g. tensor intrinsics, __mind_blas_*, cross-module imports).
            // Never a silent pass: `gate_file` turns this into a failure
            // unless `SKIP_ALLOWLIST` names the reason.
            return Checked::Skipped(format!("{e}"));
        }
    };

    // Step 2: format the source.
    let formatted = match format_source(src, &cfg) {
        Ok(s) => s,
        Err(e) => panic!("ir_preservation: format failed for {label}: {e}"),
    };

    // Step 3: compile the formatted source.
    let ir_after = ir_image(&formatted, &opts).unwrap_or_else(|e| {
        panic!(
            "ir_preservation: formatted source failed to compile for {label}: {e}\n\
                 Formatted source:\n{formatted}"
        )
    });

    // Step 4: byte-identical assertion.
    assert!(
        ir_before == ir_after,
        "canonical IR changed after formatting for {label}.\n\
         This means the formatter altered program semantics.\n\
         mic@3 diff: {}",
        describe_image_diff(&ir_before, &ir_after)
    );

    Checked::Exercised
}

// ---------------------------------------------------------------------------
// Per-file gate entry point
// ---------------------------------------------------------------------------

/// Run the gate for one in-scope file, fail-closed on every way it could
/// fail to run: unknown scope, missing file, unreadable file, or a skip that
/// is not on [`SKIP_ALLOWLIST`].
fn gate_file(rel: &str) {
    assert!(
        IN_SCOPE_FILES.contains(&rel),
        "ir_preservation: {rel} is not in IN_SCOPE_FILES — the summary would not cover it"
    );
    let path = manifest_dir().join(rel);
    assert!(
        path.exists(),
        "ir_preservation: {rel} is missing — the gate cannot be exercised"
    );
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
    let verdict = check_ir_preservation(rel, &src);

    match (allowed_skip(rel), &verdict) {
        (None, Checked::Exercised) | (Some(_), Checked::Skipped(_)) => {}
        (None, Checked::Skipped(why)) => panic!(
            "ir_preservation: {rel} was skipped, not exercised — its original source \
             failed to compile ({why}), so formatting was never checked against it. \
             Fix the compile failure, or add an explicit SKIP_ALLOWLIST entry with a \
             reason."
        ),
        (Some(reason), Checked::Exercised) => panic!(
            "ir_preservation: {rel} carries a stale SKIP_ALLOWLIST entry ({reason}) \
             but now compiles — remove the entry so the file is gated"
        ),
    }
}

// ---------------------------------------------------------------------------
// std/*.mind
// ---------------------------------------------------------------------------

#[test]
fn ir_preservation_vec() {
    gate_file("std/vec.mind");
}

#[test]
fn ir_preservation_string() {
    gate_file("std/string.mind");
}

#[test]
fn ir_preservation_io() {
    gate_file("std/io.mind");
}

#[test]
fn ir_preservation_map() {
    gate_file("std/map.mind");
}

#[test]
fn ir_preservation_blas() {
    gate_file("std/blas.mind");
}

// ---------------------------------------------------------------------------
// examples/*.mind — key self-host ladder files
// ---------------------------------------------------------------------------

#[test]
fn ir_preservation_parser_main() {
    gate_file("examples/parser/main.mind");
}

#[test]
fn ir_preservation_typecheck_main() {
    gate_file("examples/typecheck/main.mind");
}

#[test]
fn ir_preservation_emit_ir_main() {
    gate_file("examples/emit_ir/main.mind");
}

// ---------------------------------------------------------------------------
// Aggregated summary
// ---------------------------------------------------------------------------

/// Asserts the whole-scope counts, not just "something ran": every in-scope
/// file must be exercised except the explicitly allowlisted ones.
#[test]
fn ir_preservation_summary() {
    let base = manifest_dir();

    let mut exercised: Vec<&str> = Vec::new();
    let mut skipped: Vec<&str> = Vec::new();
    let mut skip_reasons: Vec<String> = Vec::new();

    for &rel in IN_SCOPE_FILES {
        let path = base.join(rel);
        assert!(
            path.exists(),
            "ir_preservation: {rel} is missing — the gate cannot be exercised"
        );
        let src =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
        match check_ir_preservation(rel, &src) {
            Checked::Exercised => exercised.push(rel),
            Checked::Skipped(why) => {
                skipped.push(rel);
                skip_reasons.push(format!("{rel}: {why}"));
            }
        }
    }

    eprintln!(
        "ir_preservation_summary: ran={} exercised={} skipped={} ({skip_reasons:?})",
        IN_SCOPE_FILES.len(),
        exercised.len(),
        skipped.len(),
    );

    let expected_skips: Vec<&str> = SKIP_ALLOWLIST.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        skipped, expected_skips,
        "ir_preservation: skipped set does not match SKIP_ALLOWLIST — a file went \
         un-exercised without a recorded reason (or an allowlisted file now compiles)"
    );
    assert_eq!(
        exercised.len(),
        IN_SCOPE_FILES.len() - SKIP_ALLOWLIST.len(),
        "ir_preservation: not every non-allowlisted in-scope file was exercised"
    );
}

// ---------------------------------------------------------------------------
// Positive control for the comparison oracle
// ---------------------------------------------------------------------------

/// A self-contained program whose only interesting content is *inside* a
/// function body, plus the single-token mutation applied to that body.
///
/// Kept next to the control test so the mutation site cannot drift away from
/// the source it mutates.
const ORACLE_CONTROL_SRC: &str =
    "pub fn ir_control_add(x: i64) -> i64 {\n    let total: i64 = x + 1;\n    total\n}\n";
const ORACLE_CONTROL_NEEDLE: &str = "x + 1";
const ORACLE_CONTROL_REPLACEMENT: &str = "x + 999";

/// Positive control: the comparison oracle must be able to SEE a semantic
/// change inside a function body.
///
/// Every other test in this file is a negative assertion (`before == after`),
/// and a negative assertion passes for free when the oracle is blind. This
/// test is the paired positive control: mutate one token inside a function
/// body and require the two images to differ. If it fails, the whole gate is
/// vacuous — the per-file tests are comparing something that does not contain
/// the program's behaviour.
#[test]
fn ir_image_oracle_sees_function_body_change() {
    let opts = default_compile_opts();
    assert!(
        ORACLE_CONTROL_SRC.contains(ORACLE_CONTROL_NEEDLE),
        "oracle control: mutation site {ORACLE_CONTROL_NEEDLE} is not in the control source"
    );
    let mutated = ORACLE_CONTROL_SRC.replacen(ORACLE_CONTROL_NEEDLE, ORACLE_CONTROL_REPLACEMENT, 1);
    assert_ne!(
        ORACLE_CONTROL_SRC, mutated,
        "oracle control: the mutation did not change the source"
    );

    let before =
        ir_image(ORACLE_CONTROL_SRC, &opts).expect("oracle control: original must compile");
    let after = ir_image(&mutated, &opts).expect("oracle control: mutant must compile");

    assert!(
        before != after,
        "oracle control: mutating `{ORACLE_CONTROL_NEEDLE}` -> `{ORACLE_CONTROL_REPLACEMENT}` \
         inside a function body produced a byte-identical image.\n\
         The IR-preservation comparison cannot see function bodies, so every \
         `before == after` assertion in this file is vacuous.\n\
         Image diff: {}",
        describe_image_diff(&before, &after)
    );
}
