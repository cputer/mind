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
//! compiled MIC IR output:
//!
//!   `emit_mic(parse(src)) == emit_mic(parse(format_source(src)))`
//!
//! This is the semantic-correctness gate: a formatter that changes the
//! program's meaning (IR) is a compiler bug, not just a style issue.
//!
//! Scope: [`IN_SCOPE_FILES`] — the single source of truth shared by the
//! per-file tests and by `ir_preservation_summary`, so the two can never
//! drift apart.
//!
//! # The gate is fail-closed on skips
//!
//! A file whose *original* source does not compile is not an IR-preservation
//! failure, but a per-file test that silently passes because its file was
//! never compiled is a gate that cannot fail. So a skip is a FAILURE unless
//! the file carries an explicit [`SKIP_ALLOWLIST`] entry naming the reason,
//! and a missing file is always a failure. `ir_preservation_summary` asserts
//! the exercised/skipped counts against that allowlist rather than merely
//! printing them.
//!
//! # What "byte-identical MIC IR" means
//!
//! The MIC (Machine Intelligence Code) IR text is produced by
//! `compile_to_mic_text`, which: parses → type-checks → lowers to IR →
//! verifies → canonicalizes → serialises.  Two sources that lower to
//! identical IR will produce byte-identical MIC text.  Formatting must
//! not rename variables, reorder top-level items, or alter the AST in
//! any way that changes the lowered result.

use libmind::fmt::format_source;
use libmind::pipeline::{CompileOptions, compile_to_mic_text};
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
// Core assertion helper
// ---------------------------------------------------------------------------

/// Assert IR-preservation for a single source string.
///
/// Returns `true` if the file was exercised (passed the assertion),
/// `false` if skipped (compile error on the original source).
///
/// `#[must_use]`: discarding the verdict is exactly how this gate went
/// vacuous — a skipped file then reads as a pass.
#[must_use]
fn check_ir_preservation(label: &str, src: &str) -> bool {
    let cfg = default_cfg();
    let opts = default_compile_opts();

    // Step 1: compile the original source.
    let ir_before = match compile_to_mic_text(src, &opts) {
        Ok(ir) => ir,
        Err(_) => {
            // Source uses features outside the compile-to-MIC scope
            // (e.g. tensor intrinsics, __mind_blas_*, cross-module imports).
            // This is expected for some files; skip without failing.
            return false;
        }
    };

    // Step 2: format the source.
    let formatted = match format_source(src, &cfg) {
        Ok(s) => s,
        Err(e) => panic!("ir_preservation: format failed for {label}: {e}"),
    };

    // Step 3: compile the formatted source.
    let ir_after = compile_to_mic_text(&formatted, &opts).unwrap_or_else(|e| {
        panic!(
            "ir_preservation: formatted source failed to compile for {label}: {e}\n\
                 Formatted source:\n{formatted}"
        )
    });

    // Step 4: byte-identical assertion.
    assert_eq!(
        ir_before, ir_after,
        "IR changed after formatting for {label}.\n\
         This means the formatter altered program semantics.\n\
         IR before formatting:\n{ir_before}\n\
         IR after formatting:\n{ir_after}",
    );

    true
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
    let exercised = check_ir_preservation(rel, &src);

    match allowed_skip(rel) {
        None => assert!(
            exercised,
            "ir_preservation: {rel} was skipped, not exercised — its original source \
             failed to compile, so formatting was never checked against it. Fix the \
             compile failure, or add an explicit SKIP_ALLOWLIST entry with a reason."
        ),
        Some(reason) => assert!(
            !exercised,
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

    for &rel in IN_SCOPE_FILES {
        let path = base.join(rel);
        assert!(
            path.exists(),
            "ir_preservation: {rel} is missing — the gate cannot be exercised"
        );
        let src =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
        if check_ir_preservation(rel, &src) {
            exercised.push(rel);
        } else {
            skipped.push(rel);
        }
    }

    eprintln!(
        "ir_preservation_summary: ran={} exercised={} skipped={} ({skipped:?})",
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
