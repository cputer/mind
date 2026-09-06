// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! `mindc check` and `mindc build` must AGREE on the two lowering refusals of
//! issue #237, and both must refuse with a structured diagnostic — never a
//! panic.
//!
//! # The defect this gate defends against
//!
//! Each refusal used to be a `panic!` reached only during lowering. `mindc
//! check` printed nothing and exited 0; `mindc <src> --emit-shared` exited 101
//! with a Rust backtrace. Two failures in one: check gave a false green, and the
//! build failure carried no code, no span and no help, so an IDE or CI saw a
//! compiler crash rather than a diagnostic.
//!
//! # What is asserted
//!
//! Per refused fixture:
//!   1. `mindc check` FAILS and names the diagnostic code;
//!   2. the build FAILS with the SAME code, exits 1 — explicitly NOT 101,
//!      because a panic is a compiler defect and not a user diagnostic;
//!   3. NO artifact is left behind.
//!
//! Plus the positives, because a gate that over-refuses is a regression, and the
//! RUNTIME controls, because "it compiled" is not "it is correct": the
//! same-binding update `out = out.push(x)` is executed across a reallocation and
//! must count every element, exactly like the bare-statement form.
//!
//! Gate: `cargo test --features "std-surface mlir-build cross-module-imports"
//!                   --test lowering_refusal_diagnostics_run`

#![cfg(all(
    unix,
    feature = "mlir-build",
    feature = "std-surface",
    feature = "cross-module-imports"
))]

mod common;

use common::{require_mindc, scratch_dir};
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGET: &str = "lowering_refusal_diagnostics_run";

/// Write `src` to a per-target, per-process scratch file and return
/// `(source, artifact)` paths. The artifact is removed first, so
/// `!artifact.exists()` afterwards is evidence about THIS run rather than about
/// whatever a previous one left on the shared temp root.
fn fixture(name: &str, src: &str) -> (PathBuf, PathBuf) {
    let dir = scratch_dir(TARGET);
    let s = dir.join(format!("{name}.mind"));
    let so = dir.join(format!("{name}.so"));
    std::fs::write(&s, src).expect("write fixture");
    let _ = std::fs::remove_file(&so);
    (s, so)
}

/// Assert that BOTH `mindc check` and the artifact-emitting build refuse
/// `src` with `code`, that the build exit status is 1 rather than a panic, and
/// that nothing was written.
fn assert_refused(name: &str, code: &str, src: &str) {
    let mindc = require_mindc();
    let (s, so) = fixture(name, src);

    let check = Command::new(&mindc)
        .args(["check", s.to_str().unwrap()])
        .output()
        .expect("run mindc check");
    let check_out = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(
        !check.status.success(),
        "{name}: `mindc check` must FAIL, but it exited 0 — the false green #237 reports:\n{check_out}"
    );
    assert!(
        check_out.contains(code),
        "{name}: `mindc check` must name {code}, got:\n{check_out}"
    );

    let build = Command::new(&mindc)
        .args([s.to_str().unwrap(), "--emit-shared", so.to_str().unwrap()])
        .output()
        .expect("run mindc build");
    let build_err = String::from_utf8_lossy(&build.stderr);
    if crate::common::gate::is_capability_gap(&build_err) {
        crate::common::gate::skipped(TARGET, &format!("{name}: needs mlir-build; skipping"));
        return;
    }
    assert_eq!(
        build.status.code(),
        Some(1),
        "{name}: the build must refuse with exit 1, not a panic (101) or success:\n{build_err}"
    );
    assert!(
        build_err.contains(code),
        "{name}: the build must name the SAME code {code} `check` did, got:\n{build_err}"
    );
    assert!(
        !so.exists(),
        "{name}: a refused compile must leave NO artifact, found {}",
        so.display()
    );
}

/// Assert `src` compiles, and return the artifact for a runtime control.
fn assert_builds(name: &str, src: &str) -> Option<PathBuf> {
    let mindc = require_mindc();
    let (s, so) = fixture(name, src);
    let out = Command::new(&mindc)
        .args([s.to_str().unwrap(), "--emit-shared", so.to_str().unwrap()])
        .output()
        .expect("run mindc");
    if !crate::common::gate::compiled(TARGET, &out) {
        return None;
    }
    let check = Command::new(&mindc)
        .args(["check", "--no-fmt", s.to_str().unwrap()])
        .output()
        .expect("run mindc check");
    let check_out = format!(
        "{}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    // The other half of "check and build agree": a source the build ACCEPTS must
    // not be refused by `check` either.
    assert!(
        !check_out.contains("E2300") && !check_out.contains("E2301"),
        "{name}: the build accepted this source but `mindc check` refused it:\n{check_out}"
    );
    Some(so)
}

/// Call `fname(arg)` in `so` and return the i64 result.
fn call1(so: &Path, fname: &str, arg: i64) -> i64 {
    let py = format!(
        "import ctypes\n\
         lib = ctypes.CDLL(r'{}')\n\
         f = lib.{fname}; f.restype = ctypes.c_int64; f.argtypes = [ctypes.c_int64]\n\
         print(f({arg}))\n",
        so.to_string_lossy()
    );
    let out = Command::new("python3")
        .args(["-c", &py])
        .output()
        .expect("python3");
    assert!(
        out.status.success(),
        "calling {fname}({arg}) failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("i64 result")
}

// ── CASE 2 — collection mutators in unrebindable positions ─────────────────

#[test]
fn nested_expression_mutator_refused_by_check_and_build() {
    assert_refused(
        "nested_expr",
        "E2300",
        "pub fn run() -> i64 {\n\
         \x20   let mut w: array<i64> = array<i64>.new()\n\
         \x20   let mut v: array<i64> = array<i64>.new()\n\
         \x20   w.push(v.push(5))\n\
         \x20   return 1\n\
         }\n",
    );
}

#[test]
fn function_argument_mutator_refused_by_check_and_build() {
    assert_refused(
        "fn_arg",
        "E2300",
        "fn id(x: i64) -> i64 { return x }\n\
         pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   return id(a.push(1))\n\
         }\n",
    );
}

#[test]
fn condition_mutator_refused_by_check_and_build() {
    for (name, body) in [
        ("cond", "if a.push(1) > 0 { return 2 }"),
        ("logical_mutator", "if a.push(1) > 0 && k > 0 { return 2 }"),
        ("cast_mutator", "return a.push(1) as i64"),
        ("unary_mutator", "return -a.push(1)"),
    ] {
        assert_refused(
            name,
            "E2300",
            &format!(
                "pub fn run(k: i64) -> i64 {{\n\
                 \x20   let mut a: array<i64> = array<i64>.new()\n\
                 \x20   {body}\n\
                 \x20   return 1\n\
                 }}\n"
            ),
        );
    }
}

#[test]
fn different_name_assign_refused_by_check_and_build() {
    // `b = a.push(1)` leaves `a` pointing at the freed handle — still refused,
    // which is what makes the same-name exemption below an exemption rather than
    // a hole.
    assert_refused(
        "diff_name_assign",
        "E2300",
        "pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let mut b: array<i64> = array<i64>.new()\n\
         \x20   b = a.push(1)\n\
         \x20   return 1\n\
         }\n",
    );
}

#[test]
fn match_scrutinee_mutator_refused_by_check_and_build() {
    assert_refused(
        "match_scrutinee",
        "E2300",
        "pub fn run(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let n = match a.push(1) { 0 => 10, _ => 20 }\n\
         \x20   return n\n\
         }\n",
    );
}

#[test]
fn match_arm_value_mutator_refused_by_check_and_build() {
    assert_refused(
        "match_arm_value",
        "E2300",
        "pub fn run(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let n = match k { 0 => a.push(1), _ => 0 }\n\
         \x20   return n\n\
         }\n",
    );
}

#[test]
fn match_arm_block_in_expression_position_refused_by_check_and_build() {
    // A braced arm body of a match used as a VALUE never meets the
    // statement-rebind pass, so `a.push(1)` there is dropped just as silently as
    // one in an argument.
    assert_refused(
        "match_arm_block",
        "E2300",
        "pub fn run(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let n = match k { 0 => { a.push(1) }, _ => { 0 } }\n\
         \x20   return n\n\
         }\n",
    );
    for (name, arm) in [
        ("match_let_init", "let x = a.push(1); 0"),
        ("match_assign", "let mut x: i64 = 0; x = a.push(1); 0"),
    ] {
        assert_refused(
            name,
            "E2300",
            &format!(
                "pub fn run(k: i64) -> i64 {{\n\
                 \x20   let mut a: array<i64> = array<i64>.new()\n\
                 \x20   let n = match k {{ 0 => {{ {arm} }}, _ => {{ 0 }} }}\n\
                 \x20   return n\n\
                 }}\n"
            ),
        );
    }
}

#[test]
fn struct_collection_field_mutator_refused_by_check_and_build() {
    assert_refused(
        "struct_field",
        "E2300",
        "struct Bag { items: array<i64> }\n\
         fn id(x: i64) -> i64 { return x }\n\
         pub fn run(b: Bag) -> i64 {\n\
         \x20   return id(b.items.push(1))\n\
         }\n",
    );
}

#[test]
fn impl_method_mutator_is_seen_by_check_not_only_by_build() {
    // The check path runs the same trait/closure desugars the build runs, so an
    // impl method body is no longer invisible to `mindc check`. Without that the
    // check exited 0 here and only the build refused — the very disagreement
    // #237 is about.
    assert_refused(
        "impl_method",
        "E2300",
        "struct Bag { items: array<i64> }\n\
         fn id(x: i64) -> i64 { return x }\n\
         trait Grow { fn grow(self) -> i64 }\n\
         impl Grow for Bag {\n\
         \x20   fn grow(self) -> i64 {\n\
         \x20       let mut t: array<i64> = array<i64>.new()\n\
         \x20       return id(t.push(1))\n\
         \x20   }\n\
         }\n\
         pub fn run() -> i64 {\n\
         \x20   let b = Bag { items: array<i64>.new() }\n\
         \x20   return b.grow()\n\
         }\n",
    );
}

#[test]
fn closure_body_mutator_is_seen_by_check_not_only_by_build() {
    // Closures are lifted to top-level fns by `desugar_closures` before BOTH
    // paths type-check, so the gate sees the body — but only because `check`
    // now runs that desugar too.
    assert_refused(
        "closure_body",
        "E2300",
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
}

#[test]
fn match_guard_mutator_refused_by_check_and_build() {
    assert_refused(
        "match_guard",
        "E2300",
        "pub fn run(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let n = match k { j if a.push(j) > 0 => 1, _ => 0 }\n\
         \x20   return n\n\
         }\n",
    );
}

#[test]
fn nested_argument_under_same_binding_rebind_refused_by_check_and_build() {
    // The same-binding exemption covers ONE node. `a = a.push(v.push(5))`
    // rebinds `a` and still drops `v`'s handle, so it must still be refused —
    // otherwise the exemption is a hole rather than a rule.
    assert_refused(
        "rebind_nested_arg",
        "E2300",
        "pub fn run() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let mut v: array<i64> = array<i64>.new()\n\
         \x20   a = a.push(v.push(5))\n\
         \x20   return 1\n\
         }\n",
    );
}

// ── CASE 3 — non-final bare identifier colliding with a variant ────────────

#[test]
fn bare_none_non_final_refused_by_check_and_build() {
    // The exact collision reported in #237 CASE 3 (mind-flow `src/sema.mind`).
    assert_refused(
        "bare_none",
        "E2301",
        "fn classify(x: i64) -> i64 {\n\
         \x20   return match x { None => 1, 0 => 2, _ => 3 };\n\
         }\n\
         fn main() -> i64 { return 0; }\n",
    );
}

// ── positives + runtime controls ───────────────────────────────────────────

#[test]
fn same_binding_update_compiles_and_counts_every_element() {
    // `out = out.push(x)` is byte-for-byte the node the rebind pass synthesises
    // for a bare `out.push(x)`. Compiling it is only half the claim; the other
    // half is that the mutation SURVIVES a reallocation, so both forms are run
    // past the initial capacity and must agree with `n`.
    let Some(so) = assert_builds(
        "same_binding_update",
        "pub fn build_assign(n: i64) -> i64 {\n\
         \x20   let mut out: array<i64> = array<i64>.new()\n\
         \x20   for i in 0..n {\n\
         \x20       out = out.push(i)\n\
         \x20   }\n\
         \x20   return out.length\n\
         }\n\
         pub fn build_stmt(n: i64) -> i64 {\n\
         \x20   let mut out: array<i64> = array<i64>.new()\n\
         \x20   for i in 0..n {\n\
         \x20       out.push(i)\n\
         \x20   }\n\
         \x20   return out.length\n\
         }\n",
    ) else {
        return;
    };
    for n in [0i64, 1, 5, 64, 257] {
        assert_eq!(
            call1(&so, "build_assign", n),
            n,
            "same-binding update lost an element at n={n} (a realloc dropped the handle)"
        );
        assert_eq!(
            call1(&so, "build_stmt", n),
            n,
            "statement-position push lost an element at n={n}"
        );
    }
}

#[test]
fn non_collection_add_is_not_refused_by_spelling() {
    // The rule is the receiver's TYPE, never the method name: a user-defined
    // `.add` on a non-collection receiver must still compile and run.
    let Some(so) = assert_builds(
        "user_add",
        "struct Counter { n: i64 }\n\
         trait Addable { fn add(self, k: i64) -> i64 }\n\
         impl Addable for Counter {\n\
         \x20   fn add(self, k: i64) -> i64 {\n\
         \x20       return self.n + k\n\
         \x20   }\n\
         }\n\
         pub fn run(k: i64) -> i64 {\n\
         \x20   let c = Counter { n: 40 }\n\
         \x20   return c.add(k)\n\
         }\n\
         pub fn set_shadowed(k: i64) -> i64 {\n\
         \x20   let mut a: set<i64> = {}\n\
         \x20   return match k { 0 => { let a: Counter = Counter { n: 40 }; a.add(2) }, _ => { 40 } }\n\
         }\n",
    ) else {
        return;
    };
    assert_eq!(
        call1(&so, "run", 2),
        42,
        "user-defined `.add` must still run"
    );
    assert_eq!(call1(&so, "set_shadowed", 0), 42);
    assert_eq!(call1(&so, "set_shadowed", 1), 40);

    let Some(shadowed) = assert_builds(
        "user_push_shadow",
        "struct Counter { n: i64 }\n\
         trait Pushable { fn push(self, k: i64) -> i64 }\n\
         impl Pushable for Counter {\n\
         \x20   fn push(self, k: i64) -> i64 { return self.n + k }\n\
         }\n\
         pub fn pushed(k: i64) -> i64 {\n\
         \x20   let c = Counter { n: 40 }\n\
         \x20   return c.push(k)\n\
         }\n\
         pub fn shadowed(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   return match k { 0 => { let a: Counter = Counter { n: 40 }; a.push(2) }, _ => { 40 } }\n\
         }\n\
         pub fn if_shadowed(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   if k == 0 { let a: Counter = Counter { n: 40 }; return a.push(2) }\n\
         \x20   return 40\n\
         }\n\
         pub fn tuple_shadowed(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let (a, b) = (Counter { n: 40 }, k)\n\
         \x20   return a.push(b)\n\
         }\n\
         pub fn tuple_branch_scope(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   if k == 0 { let (a, b) = (Counter { n: 40 }, k); a.push(2) }\n\
         \x20   a.push(7)\n\
         \x20   return a.length\n\
         }\n\
         pub fn tuple_after_outer_write(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   if k == 0 { a = a.push(7); let (a, b) = (Counter { n: 40 }, k); a.push(2) }\n\
         \x20   a.push(9)\n\
         \x20   return a.length\n\
         }\n\
         pub fn tuple_else_scope(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   if k == 0 { 0 } else { let (a, b) = (Counter { n: 40 }, k); a.push(2) }\n\
         \x20   a.push(7)\n\
         \x20   return a.length\n\
         }\n\
         pub fn tuple_while_scope(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let mut i = 0\n\
         \x20   while i < k { let (a, b) = (Counter { n: 40 }, i); a.push(2); i = i + 1 }\n\
         \x20   a.push(7)\n\
         \x20   return a.length\n\
         }\n\
         pub fn scalar_while_scope(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   let mut i = 0\n\
         \x20   while i < k { let a: Counter = Counter { n: 40 }; a.push(2); i = i + 1 }\n\
         \x20   a.push(7)\n\
         \x20   return a.length\n\
         }\n\
         pub fn tuple_region_scope(k: i64) -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   region { let (a, b) = (Counter { n: 40 }, k); a.push(2) }\n\
         \x20   a.push(7)\n\
         \x20   return a.length\n\
         }\n",
    ) else {
        return;
    };
    assert_eq!(call1(&shadowed, "pushed", 2), 42);
    assert_eq!(call1(&shadowed, "shadowed", 0), 42);
    assert_eq!(call1(&shadowed, "shadowed", 1), 40);
    assert_eq!(call1(&shadowed, "if_shadowed", 0), 42);
    assert_eq!(call1(&shadowed, "if_shadowed", 1), 40);
    assert_eq!(call1(&shadowed, "tuple_shadowed", 2), 42);
    assert_eq!(call1(&shadowed, "tuple_branch_scope", 0), 1);
    assert_eq!(call1(&shadowed, "tuple_branch_scope", 1), 1);
    assert_eq!(call1(&shadowed, "tuple_after_outer_write", 0), 2);
    assert_eq!(call1(&shadowed, "tuple_after_outer_write", 1), 1);
    assert_eq!(call1(&shadowed, "tuple_else_scope", 0), 1);
    assert_eq!(call1(&shadowed, "tuple_else_scope", 1), 1);
    assert_eq!(call1(&shadowed, "tuple_while_scope", 0), 1);
    assert_eq!(call1(&shadowed, "tuple_while_scope", 2), 1);
    assert_eq!(call1(&shadowed, "scalar_while_scope", 0), 1);
    assert_eq!(call1(&shadowed, "scalar_while_scope", 2), 1);
    assert_eq!(call1(&shadowed, "tuple_region_scope", 2), 1);
}

#[test]
fn non_final_wildcard_first_match_still_runs_correct() {
    // The truncation semantics the #306 fix introduced stay legal and correct:
    // first match wins, the unreachable `1 => 300` is never observed.
    let Some(so) = assert_builds(
        "wildcard_first_match",
        "pub fn classify(x: i64) -> i64 {\n\
         \x20   return match x { 0 => 100, _ => 200, 1 => 300 };\n\
         }\n",
    ) else {
        return;
    };
    for (arg, expected) in [(0i64, 100i64), (1, 200), (7, 200)] {
        assert_eq!(call1(&so, "classify", arg), expected, "classify({arg})");
    }
}

#[test]
fn many_modules_in_one_process_stay_stable() {
    // Repeated compilation in ONE process. The refusal sink is thread-local: if
    // a run left it armed, a later real lowering on that thread would RECORD its
    // refusal instead of panicking — the fail-closed backstop silently
    // disarmed — and a later CLEAN module could inherit a stale verdict. One
    // `mindc check` over an interleaved batch exercises exactly that, and the
    // batch includes an IMPORT so a cross-module resolve runs between refusals.
    let mindc = require_mindc();
    let dir = scratch_dir(TARGET);
    let mut paths = Vec::new();
    for (i, src) in [
        // clean
        "pub fn c0(x: i64) -> i64 {\n    return x + 1\n}\n",
        // CASE 2
        "fn id(x: i64) -> i64 { return x }\n\
         pub fn r1() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   return id(a.push(1))\n\
         }\n",
        // clean, with a match that the prefilter must let through untouched
        "pub fn c2(x: i64) -> i64 {\n    return match x { 0 => 1, _ => 2 };\n}\n",
        // CASE 3
        "pub fn r3(x: i64) -> i64 {\n    return match x { None => 1, 0 => 2, _ => 3 };\n}\n",
        // clean again — a stale sink would show up here
        "pub fn c4(x: i64) -> i64 {\n    return x * 2\n}\n",
    ]
    .iter()
    .enumerate()
    {
        let p = dir.join(format!("batch{i}.mind"));
        std::fs::write(&p, src).expect("write batch fixture");
        paths.push(p);
    }
    let mut args: Vec<String> = vec!["check".into(), "--no-fmt".into()];
    args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
    let out = Command::new(&mindc)
        .args(&args)
        .output()
        .expect("run mindc check batch");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "batch check must refuse with exit 1, not a panic:\n{text}"
    );
    assert!(
        !text.contains("panicked at"),
        "no module in the batch may panic the checker:\n{text}"
    );
    for (name, code) in [("batch1", "E2300"), ("batch3", "E2301")] {
        assert!(
            text.lines().any(|l| l.contains(name) && l.contains(code)),
            "{name} must be diagnosed {code} in the batch:\n{text}"
        );
    }
    for clean in ["batch0", "batch2", "batch4"] {
        assert!(
            !text.lines().any(|l| l.contains(clean)),
            "{clean} is clean and must produce no diagnostic — a stale sink or a \
             leaked verdict would show up here:\n{text}"
        );
    }
}

#[test]
fn imported_module_refusal_is_reported_against_its_own_file() {
    // Cross-module: the refusal lives in the IMPORTED file. Both files are
    // checked in one process and the diagnostic must be attributed to the file
    // that contains it, not to the importer.
    let mindc = require_mindc();
    let dir = scratch_dir(TARGET);
    let defs = dir.join("imp_defs.mind");
    let main = dir.join("imp_main.mind");
    std::fs::write(
        &defs,
        "fn id(x: i64) -> i64 { return x }\n\
         pub fn grow() -> i64 {\n\
         \x20   let mut a: array<i64> = array<i64>.new()\n\
         \x20   return id(a.push(1))\n\
         }\n",
    )
    .expect("write defs");
    std::fs::write(
        &main,
        "import imp_defs;\n\
         pub fn run() -> i64 {\n\
         \x20   return imp_defs.grow()\n\
         }\n",
    )
    .expect("write main");

    let out = Command::new(&mindc)
        .args([
            "check",
            "--no-fmt",
            main.to_str().unwrap(),
            defs.to_str().unwrap(),
        ])
        .output()
        .expect("run mindc check");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !text.contains("panicked at"),
        "a cross-module check must not panic:\n{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.contains("imp_defs.mind") && l.contains("E2300")),
        "the refusal must be attributed to the file that contains it:\n{text}"
    );
}

#[test]
fn qualified_enum_variants_still_run_correct() {
    let Some(so) = assert_builds(
        "qualified_variants",
        "enum Mode { On, Off }\n\
         pub fn pick(x: i64) -> i64 {\n\
         \x20   let m = if x > 0 { Mode::On } else { Mode::Off }\n\
         \x20   return match m { Mode::On => 1, Mode::Off => 0 };\n\
         }\n",
    ) else {
        return;
    };
    assert_eq!(call1(&so, "pick", 1), 1);
    assert_eq!(call1(&so, "pick", 0), 0);
}
