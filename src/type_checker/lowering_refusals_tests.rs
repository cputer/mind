// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Unit gate for `lowering_refusals`.
//!
//! Split out of the module itself so neither file approaches the 800-line
//! per-file ceiling `tests/module_size_ratchet.rs` pins, and so the rules and
//! the evidence for them stay separately readable.
//!
//! Every negative here is a POSITIVE CONTROL for the gate: it asserts the code
//! fires. Every positive asserts the gate does NOT fire, because a refusal gate
//! that over-refuses is a regression, not a safety margin.

use super::*;
use crate::parser::parse_with_diagnostics;

/// Run the gate exactly as `check_module_types_in_file` does, including the
/// trait/closure desugars the build path applies first — so a fixture that
/// puts a mutator in a closure is measured against the module lowering will
/// actually see.
fn refusals(src: &str) -> Vec<(&'static str, String)> {
    let mut module = parse_with_diagnostics(src).expect("fixture must parse");
    let _ = crate::eval::desugar_traits(&mut module, src, None);
    let _ = crate::eval::desugar_closures(&mut module, src, None);
    let mut errs = Vec::new();
    check(&module, src, None, &mut errs);
    errs.into_iter().map(|d| (d.code, d.message)).collect()
}

fn has(got: &[(&'static str, String)], code: &str) -> bool {
    got.iter().any(|(c, _)| *c == code)
}

const DECL: &str = "\x20   let mut a: array<i64> = array<i64>.new()\n";

fn in_fn(body: &str) -> String {
    format!("pub fn run() -> i64 {{\n{DECL}{body}\x20   return 1\n}}\n")
}

#[test]
fn nested_expression_mutator_is_refused() {
    let got = refusals(
        "pub fn run() -> i64 {\n\
         \x20   let mut w: array<i64> = array<i64>.new()\n\
         \x20   let mut v: array<i64> = array<i64>.new()\n\
         \x20   w.push(v.push(5))\n\
         \x20   return 1\n\
         }\n",
    );
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
    assert!(
        got.iter()
            .any(|(_, m)| m.contains("expression position") && m.contains("silently lost")),
        "the diagnostic must keep the wording the #306 gate asserts: {got:?}"
    );
}

#[test]
fn function_argument_mutator_is_refused() {
    let got = refusals(&in_fn("\x20   let n = id(a.push(1))\n"));
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn condition_mutator_is_refused() {
    for body in [
        "\x20   if a.push(1) > 0 {\n\x20       return 2\n\x20   }\n",
        "\x20   if a.push(1) > 0 && 1 > 0 {\n\x20       return 2\n\x20   }\n",
        "\x20   let n = a.push(1) as i64\n",
        "\x20   let n = -a.push(1)\n",
    ] {
        let got = refusals(&in_fn(body));
        assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
    }
}

#[test]
fn different_name_assign_is_refused() {
    let got = refusals(
        "pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let mut b: array<i64> = array<i64>.new()\n\
         \x20   b = a.push(1)\n\
         \x20   return 1\n\
         }\n",
    );
    assert!(
        has(&got, E_COLLECTION_MUTATION_IN_EXPR),
        "`b = a.push(1)` leaves `a` on the freed handle: {got:?}"
    );
}

#[test]
fn match_scrutinee_mutator_is_refused() {
    let got = refusals(&in_fn(
        "\x20   let n = match a.push(1) { 0 => 10, _ => 20 }\n",
    ));
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn match_arm_value_mutator_is_refused() {
    let got = refusals(&in_fn(
        "\x20   let n = match 1 { 0 => a.push(1), _ => 0 }\n",
    ));
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn statement_position_match_arm_value_mutator_is_refused() {
    // A match in STATEMENT position: the rebind pass only descends braced
    // arm bodies, so a bare-expression arm loses its handle just the same.
    let got = refusals(&in_fn("\x20   match 1 { 0 => a.push(1), _ => 0 }\n"));
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn match_arm_block_in_expression_position_is_refused() {
    // The block is an arm body of a match used as a VALUE, so it never meets
    // the statement-rebind pass and `a.push(1)` is dropped.
    for expr in [
        "match 1 { 0 => { a.push(1) }, _ => { 0 } }",
        "match 1 { 0 => { let x = a.push(1); 0 }, _ => { 0 } }",
        "match 1 { 0 => { let mut x: i64 = 0; x = a.push(1); 0 }, _ => { 0 } }",
    ] {
        let got = refusals(&in_fn(&format!("\x20   let n = {expr}\n")));
        assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
    }
}

#[test]
fn block_local_collection_in_expression_position_is_refused() {
    // `t` is declared INSIDE the arm block. Without threading the block's own
    // scope this receiver is untracked and the refusal is missed.
    let got = refusals(&in_fn(
        "\x20   let n = match 1 {\n\
         \x20       0 => { let mut t: array<i64> = array<i64>.new() t.push(1) t.length }\n\
         \x20       _ => { 0 }\n\
         \x20   }\n",
    ));
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn struct_collection_field_mutator_in_expr_is_refused() {
    let got = refusals(
        "struct Bag { items: array<i64> }\n\
         pub fn run(b: Bag) -> i64 {\n\
         \x20   let n = id(b.items.push(1))\n\
         \x20   return n\n\
         }\n",
    );
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

// ── positives: a gate that over-refuses is a regression ────────────────

#[test]
fn statement_position_mutation_is_not_refused() {
    assert!(refusals(&in_fn("\x20   a.push(5)\n")).is_empty());
}

#[test]
fn same_name_assign_rebind_is_not_refused() {
    // `out = out.push(x)` is the node the rebind pass itself synthesises.
    assert!(refusals(&in_fn("\x20   a = a.push(5)\n")).is_empty());
    assert!(
        refusals(&in_fn(
            "\x20   let n = match 0 { 0 => { a = a.push(5); a.length }, _ => { 0 } }\n"
        ))
        .is_empty()
    );
}

#[test]
fn same_name_let_rebind_is_not_refused() {
    assert!(refusals(&in_fn("\x20   let a = a.push(5)\n")).is_empty());
}

#[test]
fn same_name_rebind_still_inspects_its_arguments() {
    // The exemption covers ONE node — the call whose handle is written back.
    // `a = a.push(v.push(5))` rebinds `a` and still drops `v`'s handle, so the
    // nested mutator must be refused. Skipping the whole sub-tree because the
    // top was exempt would be a hole in the middle of the gate.
    for stmt in [
        "\x20   a = a.push(v.push(5))\n",
        "\x20   let a = a.push(v.push(5))\n",
    ] {
        let got = refusals(&format!(
            "pub fn run() -> i64 {{\n\
             \x20   let mut a: array<i64> = array<i64>.new()\n\
             \x20   let mut v: array<i64> = array<i64>.new()\n\
             {stmt}\
             \x20   return 1\n\
             }}\n"
        ));
        assert!(
            has(&got, E_COLLECTION_MUTATION_IN_EXPR),
            "nested argument mutator must still be refused under a same-name rebind: {got:?}"
        );
        assert_eq!(
            got.iter()
                .filter(|(c, _)| *c == E_COLLECTION_MUTATION_IN_EXPR)
                .count(),
            1,
            "only the NESTED call is refused; the rebound top call is exempt: {got:?}"
        );
    }
}

#[test]
fn statement_mutator_still_inspects_its_arguments() {
    // Same rule for the bare-statement form, which shares the helper.
    let got = refusals(
        "pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let mut v: array<i64> = array<i64>.new()\n\
         \x20   a.push(v.push(5))\n\
         \x20   return 1\n\
         }\n",
    );
    assert_eq!(
        got.iter()
            .filter(|(c, _)| *c == E_COLLECTION_MUTATION_IN_EXPR)
            .count(),
        1,
        "{got:?}"
    );
}

#[test]
fn prefilter_is_a_superset_of_the_per_match_test() {
    // The module-level prefilter's ONLY safety argument is that it never
    // excludes a match `may_refuse` would flag. Assert that directly on the
    // predicate rather than trusting the comment: any arm list the per-match
    // test accepts must also pass the registry-free prefilter, for every
    // registry.
    use crate::ast::{Node as N, Span as S};
    let span = S::new(0, 1);
    let body = || N::Lit(Literal::Int(0), span);
    let arm = |pattern: Pattern, guard: Option<N>| MatchArm {
        pattern,
        guard,
        body: body(),
        span,
    };
    let mut tags = BTreeMap::new();
    tags.insert("Option::None".to_string(), 1i64);

    let cases: Vec<Vec<MatchArm>> = vec![
        vec![],
        vec![arm(Pattern::Ident("None".into()), None)],
        vec![
            arm(Pattern::Ident("None".into()), None),
            arm(Pattern::Wildcard, None),
        ],
        vec![
            arm(Pattern::Ident("free".into()), None),
            arm(Pattern::Wildcard, None),
        ],
        vec![
            arm(Pattern::Ident("None".into()), Some(body())),
            arm(Pattern::Wildcard, None),
        ],
        vec![
            arm(Pattern::Literal(Literal::Int(0)), None),
            arm(Pattern::Wildcard, None),
        ],
    ];
    for arms in &cases {
        assert!(
            !may_refuse(arms, &tags) || has_non_final_ident_arm(arms),
            "prefilter would skip a match the per-match test flags"
        );
    }
    // …and it is a PROPER superset that still discriminates: the reported
    // collision passes both, a free binding passes only the prefilter.
    assert!(may_refuse(&cases[2], &tags) && has_non_final_ident_arm(&cases[2]));
    assert!(!may_refuse(&cases[3], &tags) && has_non_final_ident_arm(&cases[3]));
}

#[test]
fn match_free_module_needs_no_registry() {
    // The prefilter decides before any `IRModule` / prelude / global-registry
    // work. A scalar module must produce nothing, which is also the shape every
    // keystone and benchmark fixture takes.
    assert!(
        collect_match_refusals(
            &parse_with_diagnostics("pub fn run(x: i64) -> i64 {\n    return x + 1\n}\n")
                .expect("parse")
        )
        .is_empty()
    );
}

#[test]
fn repeated_gate_runs_are_stable_in_one_process() {
    // The refusal sink is thread-local. If a run left it armed, the NEXT real
    // lowering on this thread would RECORD instead of panicking — the
    // fail-closed backstop silently disarmed. Alternate refusing and clean
    // sources and assert both the verdicts and `is_collecting()` are stable.
    let bad = in_fn("\x20   let n = id(a.push(1))\n");
    let good = in_fn("\x20   a.push(5)\n");
    for _ in 0..4 {
        assert!(!is_collecting(), "sink leaked between runs");
        assert!(has(&refusals(&bad), E_COLLECTION_MUTATION_IN_EXPR));
        assert!(!is_collecting(), "sink leaked after a refusing run");
        assert!(refusals(&good).is_empty());
        assert!(!is_collecting(), "sink leaked after a clean run");
    }
}

#[test]
fn match_guard_mutator_is_refused() {
    // A guard is an ordinary expression position, and a guarded arm is NOT
    // truncated as a catch-all, so both halves must stay covered.
    let got = refusals(&in_fn(
        "\x20   let n = match 1 { k if a.push(k) > 0 => 1, _ => 0 }\n",
    ));
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn guarded_non_final_ident_arm_is_not_refused() {
    // A GUARDED irrefutable arm is refutable (the guard can fail), so arms after
    // it stay reachable and lowering handles it — refusing it would be a false
    // positive. This is the boundary of the CASE 3 rule.
    let got = refusals(
        "fn classify(x: i64) -> i64 {\n\
         \x20   return match x { None if x > 5 => 1, 0 => 2, _ => 3 };\n\
         }\n",
    );
    assert!(!has(&got, E_NON_FINAL_VARIANT_BINDING), "{got:?}");
}

#[test]
fn closure_body_mutator_is_seen_after_the_shared_desugar() {
    // Closures are lifted to top-level fns by `desugar_closures` BEFORE both the
    // check and the build type-check, so a mutator in a closure body is visible
    // to the gate. `refusals()` runs those desugars, mirroring both paths.
    let got = refusals(
        "fn id(x: i64) -> i64 { return x }\n\
         pub fn run() -> i64 {\n\
         \x20   let k: i64 = 1\n\
         \x20   let f = |k; x: i64| -> i64 {\n\
         \x20       let mut t: array<i64> = array<i64>.new()\n\
         \x20       id(t.push(x)) + k\n\
         \x20   }\n\
         \x20   return f(5)\n\
         }\n",
    );
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn impl_method_mutator_is_seen_after_the_shared_desugar() {
    let got = refusals(
        "struct Bag { items: array<i64> }\n\
         fn id(x: i64) -> i64 { return x }\n\
         trait Grow { fn grow(self) -> i64 }\n\
         impl Grow for Bag {\n\
         \x20   fn grow(self) -> i64 {\n\
         \x20       let mut t: array<i64> = array<i64>.new()\n\
         \x20       return id(t.push(1))\n\
         \x20   }\n\
         }\n",
    );
    assert!(has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn value_returning_method_is_not_refused() {
    assert!(refusals(&in_fn("\x20   let n = a.length\n")).is_empty());
}

#[test]
fn non_collection_add_is_not_refused_by_spelling() {
    // The rule is the receiver's TYPE, never the method name: a user-defined
    // `.add` on a non-collection receiver must pass through.
    let got = refusals(
        "struct Counter { n: i64 }\n\
         pub fn run(c: Counter, k: i64) -> i64 {\n\
         \x20   return c.add(k)\n\
         }\n",
    );
    assert!(!has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");
}

#[test]
fn shadowing_non_collection_add_is_not_refused() {
    // `a` is a collection in `run`; the SAME name bound to a non-collection
    // in another function must not inherit its tracking.
    let got = refusals(
        "pub fn other(a: i64) -> i64 {\n\
         \x20   return a.add(1)\n\
         }\n\
         pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   a.push(1)\n\
         \x20   return a.length\n\
         }\n",
    );
    assert!(!has(&got, E_COLLECTION_MUTATION_IN_EXPR), "{got:?}");

    let nested = refusals(
        "struct Counter { n: i64 }\n\
         pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let n = match 0 {\n\
         \x20       0 => { let a: Counter = Counter { n: 40 }; a.push(2) }\n\
         \x20       _ => { 0 }\n\
         \x20   }\n\
         \x20   return n\n\
         }\n",
    );
    assert!(!has(&nested, E_COLLECTION_MUTATION_IN_EXPR), "{nested:?}");
}

// ── CASE 3 ─────────────────────────────────────────────────────────────

#[test]
fn bare_none_in_non_final_position_is_refused() {
    // The exact collision reported in #237 CASE 3 (mind-flow src/sema.mind).
    let got = refusals(
        "fn classify(x: i64) -> i64 {\n\
         \x20   return match x { None => 1, 0 => 2, _ => 3 };\n\
         }\n",
    );
    assert!(has(&got, E_NON_FINAL_VARIANT_BINDING), "{got:?}");
}

#[test]
fn bare_none_in_final_position_is_not_refused() {
    // A trailing bare ident IS the terminal catch-all — still legal.
    let got = refusals(
        "fn classify(x: i64) -> i64 {\n\
         \x20   return match x { 0 => 1, None => 2 };\n\
         }\n",
    );
    assert!(!has(&got, E_NON_FINAL_VARIANT_BINDING), "{got:?}");
}

#[test]
fn non_final_wildcard_first_match_is_not_refused() {
    // First-match truncation semantics stay legal and unrefused.
    let got = refusals(
        "fn classify(x: i64) -> i64 {\n\
         \x20   return match x { 0 => 100, _ => 200, 1 => 300 };\n\
         }\n",
    );
    assert!(!has(&got, E_NON_FINAL_VARIANT_BINDING), "{got:?}");
}

#[test]
fn qualified_enum_variants_are_not_refused() {
    let got = refusals(
        "enum Mode { On, Off }\n\
         fn pick(m: Mode) -> i64 {\n\
         \x20   return match m { Mode::On => 1, Mode::Off => 0 };\n\
         }\n",
    );
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn dangling_variant_tag_does_not_abort_the_gate() {
    // #237 CASE 1 territory (another worker): an unknown variant path panics
    // in lowering. The gate must not inherit that panic — `mindc check` has
    // to keep returning diagnostics, not crash.
    let got = refusals(
        "enum Mode { On, Off }\n\
         fn pick(m: Mode) -> i64 {\n\
         \x20   return match m { Mode::Nope => 1, Mode::Off => 0 };\n\
         }\n",
    );
    assert!(!has(&got, E_NON_FINAL_VARIANT_BINDING), "{got:?}");
}

#[test]
fn enum_declared_after_the_function_keeps_its_catch_all() {
    // Lowering registers a top-level enum when the main loop REACHES it, so a
    // function above it does not see the variant and `None` there is still a
    // binding. Refusing it would be a false positive on code that builds.
    let got = refusals(
        "fn classify(x: i64) -> i64 {\n\
         \x20   return match x { Ready => 1, 0 => 2, _ => 3 };\n\
         }\n\
         enum State { Ready, Done }\n",
    );
    assert!(!has(&got, E_NON_FINAL_VARIANT_BINDING), "{got:?}");
}
