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

//! Q16.16 reduction-order lint contract — `E_NERVE_001` … `E_NERVE_005`.
//!
//! One POSITIVE and one NEGATIVE `.mind` fixture per rule under
//! `tests/fixtures/nerve_numerics/`, driven through the same entry points
//! `mindc check` uses, plus adversarial cases for the ways each rule could
//! diverge from the naive reading, plus the inertness proof that a unit which
//! never claims the fixed-point contract raises none of the five codes.
//!
//! The diagnostic identifiers are written here as STRING LITERALS on purpose.
//! They are a published contract consumed by a downstream numerics
//! specification, not an implementation detail: asserting against a constant
//! the implementation also owns would let a rename pass silently.

use libmind::diagnostics::Severity;

/// Absolute path of a fixture in `tests/fixtures/nerve_numerics/`.
fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nerve_numerics")
        .join(name)
}

/// Type-check `src` exactly as `mindc check` does for a single file and return
/// the ERROR-severity diagnostic codes it produced.
///
/// Error severity is part of the contract: the specification says these are
/// "compile errors, not warnings", so a rule that degraded to a warning must
/// fail these assertions rather than quietly satisfy them.
fn error_codes(src: &str, file: &str) -> Vec<String> {
    let module = libmind::parser::parse(src).expect("fixture must parse");
    let env = libmind::type_checker::TypeEnv::default();
    libmind::type_checker::check_module_types_in_file(&module, src, Some(file), &env)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.code.to_string())
        .collect()
}

/// Every diagnostic code (any severity) raised for `src`.
fn all_codes(src: &str, file: &str) -> Vec<String> {
    let module = libmind::parser::parse(src).expect("fixture must parse");
    let env = libmind::type_checker::TypeEnv::default();
    libmind::type_checker::check_module_types_in_file(&module, src, Some(file), &env)
        .into_iter()
        .map(|d| d.code.to_string())
        .collect()
}

/// The named fixture must raise `code` at error severity.
fn assert_fixture_fires(code: &str, name: &str) {
    let path = fixture_path(name);
    let src = std::fs::read_to_string(&path).expect("fixture must exist");
    let got = error_codes(&src, name);
    assert!(
        got.iter().any(|c| c == code),
        "expected error `{code}` on negative fixture `{name}`, got {got:?}"
    );
}

/// The named fixture must raise `code` at no severity at all.
fn assert_fixture_silent(code: &str, name: &str) {
    let path = fixture_path(name);
    let src = std::fs::read_to_string(&path).expect("fixture must exist");
    let got = all_codes(&src, name);
    assert!(
        !got.iter().any(|c| c == code),
        "`{code}` must not fire on positive fixture `{name}`, got {got:?}"
    );
}

/// Inline source (adversarial cases) must raise `code`.
fn assert_fires(code: &str, src: &str) {
    let got = error_codes(src, "inline.mind");
    assert!(
        got.iter().any(|c| c == code),
        "expected error `{code}`, got {got:?}\n--- source ---\n{src}"
    );
}

/// Inline source (adversarial cases) must not raise `code`.
fn assert_silent(code: &str, src: &str) {
    let got = all_codes(src, "inline.mind");
    assert!(
        !got.iter().any(|c| c == code),
        "`{code}` must not fire, got {got:?}\n--- source ---\n{src}"
    );
}

// ---------------------------------------------------------------------------
// Rule 1 — E_NERVE_001: every function reachable from a bit-identity root
//          must carry `#[determinism(BitIdentical)]`.
// ---------------------------------------------------------------------------

#[test]
fn e_nerve_001_fires_on_unannotated_reachable_callee() {
    assert_fixture_fires("E_NERVE_001", "e_nerve_001_bad.mind");
}

#[test]
fn e_nerve_001_silent_when_whole_closure_is_annotated() {
    assert_fixture_silent("E_NERVE_001", "e_nerve_001_good.mind");
}

#[test]
fn e_nerve_001_is_transitive_through_an_annotated_hop() {
    // "Rules 1-5 apply transitively": an annotated middle hop must not shield
    // an unannotated grandchild, and the error points at the leaf violation.
    let src = r#"
#[determinism(BitIdentical)]
fn preselect(x: i64) -> i64 {
    return mid(x)
}

#[determinism(BitIdentical)]
fn mid(x: i64) -> i64 {
    return leaf(x)
}

fn leaf(x: i64) -> i64 {
    return x + 1
}
"#;
    assert_fires("E_NERVE_001", src);
}

#[test]
fn e_nerve_001_ignores_a_function_off_the_reachable_closure() {
    // Adversarial: an unannotated function that no root reaches is ordinary
    // MIND. Flagging it would make the rule "annotate the whole file".
    let src = r#"
#[determinism(BitIdentical)]
fn preselect(x: i64) -> i64 {
    return x + 1
}

fn unrelated(x: i64) -> i64 {
    return x * 3
}
"#;
    assert_silent("E_NERVE_001", src);
}

// ---------------------------------------------------------------------------
// Rule 2 — E_NERVE_002: a reduction needs an explicit strategy tag.
// ---------------------------------------------------------------------------

#[test]
fn e_nerve_002_fires_on_untagged_accumulator_loop() {
    assert_fixture_fires("E_NERVE_002", "e_nerve_002_bad.mind");
}

#[test]
fn e_nerve_002_silent_with_sequential_tag() {
    assert_fixture_silent("E_NERVE_002", "e_nerve_002_good.mind");
}

#[test]
fn e_nerve_002_rejects_an_unrecognised_strategy_name() {
    // Adversarial: a tag is present but names a schedule the specification does
    // not define. Accepting any argument would let `#[reduction_strategy(fast)]`
    // buy a blanket exemption from the rule.
    let src = r#"
#[determinism(BitIdentical)]
#[reduction_strategy(fast)]
fn bad_tag(x: i64) -> i64 {
    let mut acc: i64 = x
    for i in 0..8 {
        acc = acc + i
    }
    return acc
}
"#;
    assert_fires("E_NERVE_002", src);
}

#[test]
fn e_nerve_002_accepts_the_tree_associative_fixed_schedule() {
    let src = r#"
#[determinism(BitIdentical)]
#[reduction_strategy(tree_associative_fixed)]
fn tree_sum(x: i64) -> i64 {
    let mut acc: i64 = x
    for i in 0..8 {
        acc = acc + i
    }
    return acc
}
"#;
    assert_silent("E_NERVE_002", src);
}

#[test]
fn e_nerve_002_ignores_a_loop_that_is_not_a_reduction() {
    // Adversarial: a loop whose assignment never reads its own target is a
    // scatter/store, not a fold. Requiring a tag there turns the rule into
    // noise, and noise gets suppressed.
    let src = r#"
#[determinism(BitIdentical)]
fn scatter(n: i64) -> i64 {
    let mut last: i64 = n
    for i in 0..8 {
        last = i
    }
    return last
}
"#;
    assert_silent("E_NERVE_002", src);
}

#[test]
fn e_nerve_002_catches_a_running_max_select() {
    // The select shape: the running max/min/argmax depends on visit order and
    // tie-break order just as much as an accumulate does.
    let src = r#"
#[determinism(BitIdentical)]
fn running_max(n: i64) -> i64 {
    let mut best: i64 = n
    for i in 0..8 {
        if i > best {
            best = i
        }
    }
    return best
}
"#;
    assert_fires("E_NERVE_002", src);
}

#[test]
fn e_nerve_002_over_approximates_an_explicit_induction_step_on_purpose() {
    // An explicit `while` counter reads its own target, so it is reported even
    // though a counter is order-pinned by construction. Pinned as a DECISION,
    // not an accident: the fail-closed direction costs one truthful tag,
    // whereas carving out self-increment would have to distinguish a counter
    // from `acc = acc + x[i]` under `while acc < limit` — and getting that
    // wrong is a silent miss on a bit-identity gate.
    let src = r#"
#[determinism(BitIdentical)]
fn count_up(n: i64) -> i64 {
    let mut i: i64 = 0
    while i < n {
        i = i + 1
    }
    return i
}
"#;
    assert_fires("E_NERVE_002", src);
}

// ---------------------------------------------------------------------------
// Rule 3 — E_NERVE_003: the only legal Q16.16 multiply is `q16_mul`.
// ---------------------------------------------------------------------------

#[test]
fn e_nerve_003_fires_on_hand_rolled_shift_16_multiply() {
    assert_fixture_fires("E_NERVE_003", "e_nerve_003_bad.mind");
}

#[test]
fn e_nerve_003_silent_when_routed_through_q16_mul() {
    assert_fixture_silent("E_NERVE_003", "e_nerve_003_good.mind");
}

#[test]
fn e_nerve_003_sees_through_widening_casts() {
    // The specification's own example spells the scale correction with casts:
    // `(a as i64 * b as i64 >> 16) as i32`. The casts must not hide it.
    let src = r#"
#[determinism(BitIdentical)]
fn scale(a: i32, b: i32) -> i32 {
    return ((a as i64 * b as i64) >> 16) as i32
}
"#;
    assert_fires("E_NERVE_003", src);
}

#[test]
fn e_nerve_003_fires_on_a_raw_multiply_of_q16_typed_values() {
    // The other half of the rule: no `>> 16` in sight, but both operands are
    // declared Q16.16, so the scale correction is MISSING — an error the type
    // system cannot see because the fixed-point-ness lives in a type alias.
    let src = r#"
type Q16_16 = i64

#[determinism(BitIdentical)]
fn scale(a: Q16_16, b: Q16_16) -> Q16_16 {
    return a * b
}
"#;
    assert_fires("E_NERVE_003", src);
}

#[test]
fn e_nerve_003_ignores_a_plain_integer_shift() {
    // Adversarial: `>> 16` with no multiply beneath it is a field extract, not
    // a fixed-point multiply.
    let src = r#"
#[determinism(BitIdentical)]
fn hi(a: i64) -> i64 {
    return a >> 16
}
"#;
    assert_silent("E_NERVE_003", src);
}

#[test]
fn e_nerve_003_ignores_an_integer_multiply_of_untyped_values() {
    // Adversarial: a plain integer multiply on values never declared Q16.16 is
    // ordinary arithmetic and carries no scale obligation.
    let src = r#"
#[determinism(BitIdentical)]
fn area(w: i64, h: i64) -> i64 {
    return w * h
}
"#;
    assert_silent("E_NERVE_003", src);
}

// ---------------------------------------------------------------------------
// Rule 4 — E_NERVE_004: no IEEE-754 under `#[invariant(no_float_ops)]`.
// ---------------------------------------------------------------------------

#[test]
fn e_nerve_004_fires_on_a_float_binding() {
    assert_fixture_fires("E_NERVE_004", "e_nerve_004_bad.mind");
}

#[test]
fn e_nerve_004_silent_on_an_all_integer_body() {
    assert_fixture_silent("E_NERVE_004", "e_nerve_004_good.mind");
}

#[test]
fn e_nerve_004_fires_on_a_float_signature() {
    let src = r#"
#[invariant(no_float_ops)]
#[determinism(BitIdentical)]
fn norm(raw: f32) -> f32 {
    return raw
}
"#;
    assert_fires("E_NERVE_004", src);
}

#[test]
fn e_nerve_004_fires_on_a_cast_into_ieee754() {
    // A float cannot enter by type, by value, by conversion, or by result; the
    // cast surface is the one a signature-only check would miss.
    let src = r#"
#[invariant(no_float_ops)]
#[determinism(BitIdentical)]
fn norm(raw: i64) -> i64 {
    let s: i64 = (raw as f32) as i64
    return s
}
"#;
    assert_fires("E_NERVE_004", src);
}

#[test]
fn e_nerve_004_is_not_armed_without_the_invariant() {
    // Adversarial: `#[determinism(BitIdentical)]` alone arms rules 1/2/3/5 but
    // NOT rule 4 — the no-float invariant is a separate, explicit claim.
    let src = r#"
#[determinism(BitIdentical)]
fn widen(x: f64) -> f64 {
    return x
}
"#;
    assert_silent("E_NERVE_004", src);
}

#[test]
fn e_nerve_004_does_not_trust_the_name_of_a_locally_defined_helper() {
    // Adversarial (name-trust): the contract MANDATES a fixed-point unit ship
    // its own truncated-lookup `exp`. Rejecting the call on its name would ban
    // the very implementation the contract requires; a definition carried in
    // this file is judged by its own body instead.
    let src = r#"
#[invariant(no_float_ops)]
#[determinism(BitIdentical)]
fn exp(x: i64) -> i64 {
    return x + 65536
}

#[invariant(no_float_ops)]
#[determinism(BitIdentical)]
fn shifted(x: i64) -> i64 {
    return exp(x)
}
"#;
    assert_silent("E_NERVE_004", src);
}

// ---------------------------------------------------------------------------
// Rule 5 — E_NERVE_005: implementation-defined reductions are rejected even
//          when the caller declares a strategy tag.
// ---------------------------------------------------------------------------

#[test]
fn e_nerve_005_fires_on_a_backend_reduction_despite_a_strategy_tag() {
    assert_fixture_fires("E_NERVE_005", "e_nerve_005_bad.mind");
}

#[test]
fn e_nerve_005_silent_on_an_in_source_pinned_fold() {
    assert_fixture_silent("E_NERVE_005", "e_nerve_005_good.mind");
}

#[test]
fn e_nerve_005_does_not_trust_the_name_of_a_locally_defined_fold() {
    // Adversarial (name-trust): a function the unit DEFINES itself is governed
    // by rules 1-3 whatever it is called. Rejecting it on its name alone would
    // ban `fn reduce_sum` from ever being written in MIND, and would report a
    // pinned in-source fold as implementation-defined.
    let src = r#"
#[determinism(BitIdentical)]
#[reduction_strategy(sequential)]
fn reduce_sum(x: i64) -> i64 {
    let mut acc: i64 = 0
    for i in 0..8 {
        acc = acc + i
    }
    return acc + x
}

#[determinism(BitIdentical)]
fn total(x: i64) -> i64 {
    return reduce_sum(x)
}
"#;
    assert_silent("E_NERVE_005", src);
}

// ---------------------------------------------------------------------------
// Opt-in inertness — every other MIND unit must be unaffected.
// ---------------------------------------------------------------------------

#[test]
fn every_rule_is_inert_without_an_opt_in_annotation() {
    let name = "unarmed_all_violations.mind";
    let src = std::fs::read_to_string(fixture_path(name)).expect("fixture must exist");
    let got = all_codes(&src, name);
    for code in [
        "E_NERVE_001",
        "E_NERVE_002",
        "E_NERVE_003",
        "E_NERVE_004",
        "E_NERVE_005",
    ] {
        assert!(
            !got.iter().any(|c| c == code),
            "`{code}` fired on an un-annotated unit — the lint is not opt-in; got {got:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// End-to-end: the codes reach the `mindc check` exit code.
// ---------------------------------------------------------------------------

/// Run `mindc check`'s library entry point over one fixture and return its
/// process exit code.
fn check_exit_code(name: &str) -> i32 {
    let opts = libmind::check::CheckOptions {
        run_fmt: false,
        run_lint: false,
        run_typecheck: true,
        reporter: libmind::check::ReporterKind::Json,
        paths: vec![fixture_path(name).display().to_string()],
        fix: false,
    };
    libmind::check::run_check(&opts)
}

#[test]
fn mindc_check_rejects_every_negative_fixture() {
    for name in [
        "e_nerve_001_bad.mind",
        "e_nerve_002_bad.mind",
        "e_nerve_003_bad.mind",
        "e_nerve_004_bad.mind",
        "e_nerve_005_bad.mind",
    ] {
        assert_eq!(
            check_exit_code(name),
            1,
            "`mindc check` must exit non-zero on negative fixture `{name}`"
        );
    }
}

#[test]
fn mindc_check_accepts_every_positive_fixture() {
    for name in [
        "e_nerve_001_good.mind",
        "e_nerve_002_good.mind",
        "e_nerve_003_good.mind",
        "e_nerve_004_good.mind",
        "e_nerve_005_good.mind",
        "unarmed_all_violations.mind",
    ] {
        assert_eq!(
            check_exit_code(name),
            0,
            "`mindc check` must exit clean on positive fixture `{name}`"
        );
    }
}
