// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! RFC 0010 §4.2 — characterization gate for the `safe` / `unsafe` extern tag.
//!
//! These tests do NOT assert that the safety model works. They assert the
//! opposite: that the tag is currently INFORMATIONAL and that raw pointers are
//! admitted with no `unsafe` context, because the language has no `unsafe`
//! block construct to require. They exist so the gap stays visible and so the
//! source docs and the implementation cannot drift apart silently — the doc
//! comments on `ast::TypeAnn::RawPtr` and `ast::ExternFn::is_unsafe` once
//! claimed the restriction was enforced when nothing enforced it.
//!
//! When the `unsafe { … }` block and its context-tracking pass land, these
//! tests MUST be inverted in the same change that updates those doc comments
//! and the RFC 0010 §4.2 status note. A red test here is the reminder.
//!
//! Gated: `cargo test --features "std-surface" --test
//!         extern_c_safety_tag_informational`.

#![cfg(feature = "std-surface")]

use libmind::ast::Node;
use libmind::parser;
use libmind::type_checker::{TypeEnv, check_module_types_in_file};

/// The program from the F8 audit finding: no `unsafe` token anywhere, a
/// pointer-taking libc symbol declared `safe fn`, called with a raw integer.
const F8_REPRO: &str = r#"extern "C" {
    safe fn system(cmd: *const u8) -> i32;
}

fn main() -> i64 {
    let addr = 140737488355328;
    let rc = system(addr);
    return rc;
}
"#;

/// A raw pointer in ordinary (non-`extern`) type positions: a plain `fn`
/// parameter and a `let` annotation, both fed a bare integer literal.
const NON_EXTERN_PTR_POSITIONS: &str = r#"fn deref_it(p: *mut i64) -> i64 {
    return 7;
}

fn main() -> i64 {
    let q: *const u8 = 140737488355328;
    return deref_it(140737488355328);
}
"#;

fn diags_for(src: &str) -> String {
    let module = parser::parse(src).unwrap_or_else(|e| panic!("parse failed: {e:?}"));
    let diags = check_module_types_in_file(&module, src, None, &TypeEnv::default());
    diags.iter().map(|d| format!("{d:?}")).collect()
}

/// The parser records the tag — that much of RFC 0010 §4.2 IS implemented.
#[test]
fn safety_tag_is_recorded_by_the_parser() {
    let module = parser::parse(F8_REPRO).unwrap_or_else(|e| panic!("parse failed: {e:?}"));
    let Node::ExternBlock { fns, .. } = &module.items[0] else {
        panic!("expected ExternBlock as the first item")
    };
    assert_eq!(fns[0].name, "system");
    assert!(
        !fns[0].is_unsafe,
        "`safe fn system` must parse with is_unsafe = false"
    );
}

/// …and nothing downstream acts on it. Declaring a pointer-taking libc symbol
/// `safe fn` and calling it with an integer produces no diagnostic at all.
///
/// deferred: this is the hole, not the goal. Invert this test when the
/// `unsafe` context lands.
#[test]
fn safety_tag_is_not_enforced_by_the_type_checker() {
    let combined = diags_for(F8_REPRO);
    assert!(
        !combined.contains("unsafe"),
        "unexpected `unsafe`-related diagnostic — if enforcement has landed, \
         invert this test AND update the `deferred:` docs on \
         `ast::TypeAnn::RawPtr` / `ast::ExternFn::is_unsafe` and the RFC 0010 \
         §4.2 status note in the same change; got: {combined}"
    );
}

/// Raw pointers are parsed by the general `type_ann`, so they are legal in
/// every type position, not only `extern "C"` signatures.
///
/// deferred: same hole; invert together with the test above.
#[test]
fn raw_pointers_are_admitted_outside_extern_signatures() {
    let combined = diags_for(NON_EXTERN_PTR_POSITIONS);
    assert!(
        !combined.contains("unsafe"),
        "unexpected `unsafe`-related diagnostic on non-extern pointer \
         positions — see the note on the test above; got: {combined}"
    );
}

/// Doc-honesty gate: the retracted claim must not come back.
///
/// `src/ast/mod.rs` asserted "Raw pointers are only constructible inside
/// `unsafe` blocks" while no such block existed and nothing checked anything.
/// This test fails if that sentence reappears, and requires the `deferred:`
/// marker that replaced it to stay until the gap is actually closed.
#[test]
fn ast_docs_do_not_reassert_unenforced_pointer_safety() {
    let ast_src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/ast/mod.rs"))
        .expect("src/ast/mod.rs must be readable from the manifest dir");
    // Normalise doc-comment prefixes and line wrapping so the check does not
    // depend on where the sentence happens to break across lines.
    let flat: String = ast_src
        .lines()
        .map(|l| l.trim().trim_start_matches("///").trim())
        .collect::<Vec<_>>()
        .join(" ");
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        !flat.contains("Raw pointers are only constructible inside `unsafe` blocks"),
        "src/ast/mod.rs re-asserts an unenforced safety restriction. There is \
         no `unsafe` block construct in the language and no pass checks raw \
         pointer construction, so this claim is false. State what the compiler \
         actually does and keep a `deferred:` marker for the gap."
    );
    assert!(
        flat.contains("deferred:") && flat.contains("`unsafe` BLOCK construct at all"),
        "the `deferred:` marker on `ast::TypeAnn::RawPtr` is missing. If the \
         `unsafe` context enforcement has actually landed, delete this \
         assertion in the same change that deletes the marker, and invert the \
         two behavioural tests above."
    );
}
