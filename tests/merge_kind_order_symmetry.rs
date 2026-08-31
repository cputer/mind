//! A merge's signedness must not depend on ARM ORDER.
//!
//! `unify_merge_kind` returned `then_kind` whenever both arms shared an MLIR type,
//! discarding `else_kind`. ScalarU64 and ScalarI64 both map to "i64", so for `x: u64`:
//!
//!     if c { 1 } else { x }   emitted shrsi   (SIGNED)
//!     if c { x } else { 1 }   emitted shrui   (UNSIGNED)
//!
//! Same program, same types, different arithmetic, decided by which arm was written
//! first. That is a determinism defect before it is a signedness defect: for a compiler
//! whose claim is order-independent byte-identity, a join that is not commutative is
//! already wrong.
//!
//! These tests assert the INVARIANT (both orders agree), not a specific spelling, so
//! they keep holding if the chosen op variants ever change for another reason.

use std::process::Command;

fn emit_mlir(src: &str, stem: &str) -> String {
    let dir = std::env::temp_dir().join(format!("mind_merge_sym_{stem}"));
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("case.mind");
    std::fs::write(&f, src).expect("write fixture");
    let bin = env!("CARGO_BIN_EXE_mindc");
    let out = Command::new(bin)
        .args(["--emit-mlir", f.to_str().unwrap()])
        .output()
        .expect("spawn mindc");
    assert!(
        out.status.success(),
        "mindc --emit-mlir failed for {stem}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Ops whose MLIR spelling encodes signedness. Comparing the multiset of these across
/// both arm orders is the assertion.
fn signed_sensitive_ops(mlir: &str) -> Vec<String> {
    const OPS: [&str; 8] = [
        "shrsi", "shrui", "divsi", "divui", "remsi", "remui", "extsi", "extui",
    ];
    let mut found: Vec<String> = OPS
        .iter()
        .flat_map(|op| std::iter::repeat(op.to_string()).take(mlir.matches(op).count()))
        .collect();
    found.sort_unstable();
    found
}

fn assert_order_symmetric(ty: &str, op: &str, stem: &str) {
    // The merged value flows DIRECTLY into the operation. It must NOT be bound through
    // a `let v: u64 = ...` first: that annotation makes `mask_narrow_let` re-apply the
    // __mind_conv_u64 tag to the merge RESULT, repairing the kind whatever the merge
    // decided -- so the bound form passes on a BROKEN compiler and tests the annotation
    // rather than the join. Measured: the first version of this test passed with the
    // arm-pick bug deliberately reinstated. This form does not.
    //   arm-pick:    if c {1} else {x} -> shrsi   |  if c {x} else {1} -> shrui
    //   absorption:  both -> shrui
    let lit_then = format!(
        "pub fn f(c: i64, x: {ty}) -> i64 {{\n    return ((if c == 1 {{ 1 }} else {{ x }}) {op}) as i64;\n}}\n"
    );
    let lit_else = format!(
        "pub fn f(c: i64, x: {ty}) -> i64 {{\n    return ((if c == 1 {{ x }} else {{ 1 }}) {op}) as i64;\n}}\n"
    );
    let a = signed_sensitive_ops(&emit_mlir(&lit_then, &format!("{stem}_then")));
    let b = signed_sensitive_ops(&emit_mlir(&lit_else, &format!("{stem}_else")));
    assert!(
        !a.is_empty(),
        "{stem}: no signedness-sensitive op was emitted at all -- the fixture does not \
         reach the merge, so this assertion would hold vacuously. Fix the fixture."
    );
    assert_eq!(
        a, b,
        "arm ORDER changed the emitted signed/unsigned ops for {ty}: \
         literal-then={a:?} literal-else={b:?}. A merge kind must be a commutative join."
    );
}

#[test]
fn u64_merge_is_order_independent_shift() {
    assert_order_symmetric("u64", ">> 63", "u64_shr");
}

#[test]
fn u64_merge_is_order_independent_div() {
    assert_order_symmetric("u64", "/ 2", "u64_div");
}

#[test]
fn u32_merge_is_order_independent_shift() {
    assert_order_symmetric("u32", ">> 31", "u32_shr");
}
