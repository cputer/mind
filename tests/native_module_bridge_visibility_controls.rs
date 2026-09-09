//! Native module-bridge controls: VISIBILITY, EMIT KIND, CROSS-INVOCATION
//! IDENTITY and REFUSAL SHAPE.
//!
//! The export boundary and its parity with `mindc check`, what each `--emit`
//! kind is allowed to write, how the verdict tracks the source each invocation
//! captured, and that refusals arrive as diagnostics carrying their stable kind
//! rather than as aborts. Resolution and admission controls live in
//! `native_module_bridge_controls.rs`; the two files share
//! `tests/native_bridge_support/`.
//!
//! On Linux x86-64 positive cases execute the real native image. Other hosts
//! execute the host-native drain fixture and still exercise visibility, refusal,
//! artifact, and complete image-transport controls.
//!
//! Split purely to stay under the 800-line ceiling. Source fixtures and refusal
//! assertions are unchanged; positive result checks use the target-aware helper.
#![cfg(all(feature = "cross-module-imports", feature = "std-surface"))]

mod native_bridge_support;
use native_bridge_support::{Project, assert_native_result};

#[test]
fn unexported_callee_is_refused_in_parity_with_check() {
    let p = Project::new("exportgate");
    p.write(
        "src/helper.mind",
        "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         fn secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\nfn main() -> i64 {\n    return secret(5);\n}\n",
    );
    let (code, err, artifact) = p.build_native("src/main.mind");
    assert_ne!(code, 0, "an unexported callee must refuse: {err}");
    assert!(artifact.is_none(), "refusal must write no artifact");
    assert!(
        err.contains("scope.rejected_by_type_check"),
        "refusal must be OWNED by the type-check parity gate, not an incidental later \
         failure: {err}"
    );
}

#[test]
fn exported_callee_still_builds() {
    let p = Project::new("exportok");
    p.write(
        "src/helper.mind",
        "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         fn secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\nfn main() -> i64 {\n    return public_fn(6);\n}\n",
    );
    let (code, err, artifact) = p.build_native("src/main.mind");
    assert_eq!(code, 0, "the exported callee must still build: {err}");
    let bytes = artifact.expect("artifact");
    assert_native_result(&p, &bytes, 7, "exported callee result");
}

#[test]
fn sibling_owned_main_is_refused() {
    let p = Project::new("sibmain");
    p.write(
        "src/helper.mind",
        "pub fn helper() -> i64 {\n    return 1;\n}\n\nfn main() -> i64 {\n    return 42;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\npub fn entry_only(x: i64) -> i64 {\n    return helper() + x;\n}\n",
    );
    let (code, err, artifact) = p.build_native("src/main.mind");
    assert_ne!(code, 0, "an entry with no main must refuse: {err}");
    assert!(artifact.is_none(), "must not build the sibling's program");
    assert!(
        err.contains("scope.entry_module_has_no_main"),
        "refusal must be owned by the entry-root rule: {err}"
    );
}

#[test]
fn transitive_unexported_callee_is_refused() {
    let p = Project::new("transitive");
    p.write(
        "src/helper.mind",
        "export { relayed }\n\npub fn relayed(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         fn deep_secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    );
    p.write(
        "src/relay.mind",
        "import helper;\n\npub fn hop(x: i64) -> i64 {\n    return relayed(x);\n}\n",
    );
    p.write(
        "src/main.mind",
        "import relay;\n\nfn main() -> i64 {\n    return deep_secret(5);\n}\n",
    );
    let (code, err, artifact) = p.build_native("src/main.mind");
    assert_ne!(code, 0, "a transitive unexported callee must refuse: {err}");
    assert!(
        artifact.is_none(),
        "must not build what the language refuses"
    );
    // Named kind, MEASURED: `scope.rejected_by_type_check`. Without this the
    // control passed on ANY nonzero exit -- a fixture typo or an unrelated
    // failure would have satisfied it just as well.
    assert!(
        err.contains("scope.rejected_by_type_check"),
        "the refusal must be owned by the type checker, which is the authority \
         on visibility, not by whatever else might fail: {err}"
    );
}

#[test]
fn a_comment_mentioning_a_call_does_not_refuse() {
    let p = Project::new("commentonly");
    p.write(
        "src/helper.mind",
        "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         fn secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\nfn main() -> i64 {\n    // does not call secret(5) here\n    \
         return public_fn(6);\n}\n",
    );
    let (code, err, artifact) = p.build_native("src/main.mind");
    assert_eq!(code, 0, "a comment is not a call; this must build: {err}");
    let bytes = artifact.expect("artifact");
    assert_native_result(&p, &bytes, 7, "comment-only call result");
}

#[test]
fn library_emit_is_refused_not_mislabelled() {
    let p = Project::new("emitlib");
    p.write("src/main.mind", "fn main() -> i64 {\n    return 7;\n}\n");
    let (code, err, artifact) = p.build_native_emit("src/main.mind", "cdylib");
    assert_ne!(
        code, 0,
        "a library emit must refuse on the native backend: {err}"
    );
    assert!(
        artifact.is_none(),
        "must not write an executable under a library name"
    );
    assert!(
        err.contains("--emit=cdylib"),
        "diagnostic must name the emit kind: {err}"
    );
}

#[test]
fn executable_emit_still_builds() {
    let p = Project::new("emitbin");
    p.write("src/main.mind", "fn main() -> i64 {\n    return 7;\n}\n");
    let (code, err, artifact) = p.build_native_emit("src/main.mind", "binary");
    assert_eq!(code, 0, "{err}");
    let bytes = artifact.expect("artifact");
    assert_native_result(&p, &bytes, 7, "executable emit result");
}

#[test]
fn captured_private_still_refuses_when_disk_is_made_public() {
    let p = Project::new("snap_priv");
    p.write(
        "src/helper.mind",
        "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         fn secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\nfn main() -> i64 {\n    return secret(5);\n}\n",
    );

    // Racing the compiler is not a test. Instead, prove the bytes the build used
    // are the bytes on disk AT INVOCATION, then show the same build refuses while
    // a later disk state would have been legal.
    let (code, err, artifact) = p.build_native("src/main.mind");
    assert_ne!(code, 0, "captured bytes carry the violation: {err}");
    assert!(artifact.is_none());
    assert!(err.contains("scope.rejected_by_type_check"), "{err}");

    // Now make the disk version legal. A rebuild MUST now succeed -- proving the
    // refusal above tracked source content and is not a sticky/cached verdict.
    p.write(
        "src/helper.mind",
        "export { public_fn, secret }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         pub fn secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    );
    let (code2, err2, art2) = p.build_native("src/main.mind");
    assert_eq!(code2, 0, "the now-legal program must build: {err2}");
    let bytes = art2.expect("artifact");
    assert_native_result(&p, &bytes, 105, "captured-private legal result");
}

#[test]
fn removed_dependency_refuses() {
    let p = Project::new("snap_rm");
    p.write(
        "src/helper.mind",
        "pub fn helper(x: i64) -> i64 {\n    return x + 1;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\nfn main() -> i64 {\n    return helper(6);\n}\n",
    );
    let (ok_code, err, art) = p.build_native("src/main.mind");
    assert_eq!(ok_code, 0, "positive control must build first: {err}");
    let bytes = art.expect("artifact");
    assert_native_result(&p, &bytes, 7, "removed-dependency positive result");

    std::fs::remove_file(p.root().join("src/helper.mind")).expect("remove dependency");
    let (code, err2, artifact) = p.build_native("src/main.mind");
    assert_ne!(code, 0, "a removed dependency must refuse: {err2}");
    assert!(artifact.is_none(), "must not emit from a stale table");
    // Named kind, MEASURED: `scope.unresolved_import`. The second invocation
    // must fail because the import no longer resolves, not because something
    // else happened to break after the file was deleted.
    assert!(
        err2.contains("scope.unresolved_import"),
        "the refusal must be owned by import resolution: {err2}"
    );
}

#[test]
fn retargeted_dependency_changes_the_verdict() {
    let p = Project::new("snap_retarget");
    p.write(
        "src/helper.mind",
        "pub fn helper(x: i64) -> i64 {\n    return x + 1;\n}\n",
    );
    p.write(
        "src/main.mind",
        "import helper;\n\nfn main() -> i64 {\n    return helper(6);\n}\n",
    );
    let (a, _, art_a) = p.build_native("src/main.mind");
    assert_eq!(a, 0);
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    let captured_a = {
        let captured = p.captured_source_image();
        assert!(
            captured
                .windows(b"return x + 1".len())
                .any(|window| window == b"return x + 1"),
            "initial source image must contain the original dependency body"
        );
        captured
    };
    let bytes = art_a.expect("artifact");
    assert_native_result(&p, &bytes, 7, "initial dependency result");

    // Same module name, different body: 6 + 10 = 16, not 7.
    p.write(
        "src/helper.mind",
        "pub fn helper(x: i64) -> i64 {\n    return x + 10;\n}\n",
    );
    let (b, err, art_b) = p.build_native("src/main.mind");
    assert_eq!(b, 0, "{err}");
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let captured_b = p.captured_source_image();
        assert_ne!(
            captured_a, captured_b,
            "retargeting must change the source image received by the host fixture"
        );
        assert!(
            captured_b
                .windows(b"return x + 10".len())
                .any(|window| window == b"return x + 10"),
            "retargeted source image must contain the new dependency body"
        );
        assert!(
            !captured_a
                .windows(b"return x + 10".len())
                .any(|window| window == b"return x + 10"),
            "initial source image must not contain the retargeted body"
        );
    }
    let bytes = art_b.expect("artifact");
    assert_native_result(
        &p,
        &bytes,
        16,
        "the emitted artifact must reflect the retargeted dependency, not a cached one",
    );
}

#[test]
fn a_cross_module_enum_refuses_cleanly_instead_of_aborting() {
    let p = Project::new("xenum");
    p.write(
        "src/palette.mind",
        "pub enum Color {\n    Red,\n    Green,\n}\n\n\
         pub fn code(c: Color) -> i64 {\n    match c {\n        \
         Color.Red => { return 7; }\n        Color.Green => { return 9; }\n    }\n}\n",
    );
    p.write(
        "src/main.mind",
        "import palette;\n\nfn main() -> i64 {\n    return code(Color.Red);\n}\n",
    );
    let (code, err, art) = p.build_native("src/main.mind");
    assert_ne!(code, 0, "an out-of-profile construct must refuse: {err}");
    assert!(
        !err.contains("panicked") && !err.contains("RUST_BACKTRACE"),
        "the refusal must be a diagnostic, not an abort: {err}"
    );
    assert!(
        err.contains("error[backend-native]"),
        "the refusal must be reported as a native-backend diagnostic: {err}"
    );
    assert!(
        art.is_none(),
        "a refused build writes no artifact (fail-closed)"
    );
}

/// A cross-module ENUM must produce a DIAGNOSTIC, never a process abort.
///
/// This is the gate for the captured scope's table being installed around the
/// merged lowering. It is written as a regression control because the failure it
/// covers was measured, not imagined: before the guard existed, `Color.Red`
/// declared in a sibling and used from the entry reached the
/// "refusing to emit const 0 (a silent miscompile)" panic in `lower.rs` and
/// aborted the process with a Rust backtrace. The SAME construct in ONE file
/// refused cleanly through the frozen compiler, so the multi-module path was
/// strictly worse than the single-file path it was modelled on.
///
/// MUTATION (measured): deleting the `_scope_guard` binding in
/// `native_bridge.rs` turns this test red -- lowering loses the enum table, the
/// receiver goes unresolved, and the panic returns. That is what makes
/// `scope.install()` load bearing here rather than merely argued.
///
/// The enum itself is out of the native profile today, so the ASSERTION is on
/// the SHAPE of the failure, not on a successful build: refuse, name the
/// construct, write nothing, and stay a diagnostic. If the profile later admits
/// enums this test should start asserting a successful run instead of a refusal
/// -- but it must never accept an abort.
/// The `lower.materialization_refused` marker is REACHED and reported, not just
/// written.
///
/// Root's review found no control asserting this kind. The enum control above is
/// principally a captured-scope-guard test: with the guard present the sibling
/// receiver resolves, so it exercises a later generic refusal and never proves
/// this branch ran. This one does, by making `lower_to_ir` actually return a
/// `MaterializationRefusal`.
///
/// Trigger, measured: a fixed-array struct field of 300_000 elements charges
/// 2_400_000 payload bytes against the 2_097_152 default limit, so lowering
/// refuses with `PayloadLimit` before the closure or profile fences are reached.
/// The source is GENERATED here rather than committed: it is ~0.6 MB of literals
/// and has no business in the tree.
///
/// MUTATION: replace the `map_err` in `admit_merged_program` with an `unwrap()`
/// or `unwrap_or_default()` and this goes red -- an unwrap aborts (no
/// `error[backend-native]`, and `panicked` appears), a default substitutes an
/// empty module and the build wrongly succeeds.
#[test]
fn a_lowering_refusal_is_reported_with_its_stable_kind() {
    const N: usize = 300_000;
    let mut elems = String::with_capacity(N * 2);
    for i in 0..N {
        if i > 0 {
            elems.push(',');
        }
        elems.push('0');
    }
    let src = format!(
        "struct S {{\n    a: [i64; {N}],\n}}\n\n\
         fn main() -> i64 {{\n    let s = S {{ a: [{elems}] }};\n    return s.a[0];\n}}\n"
    );

    let p = Project::new("matrefuse");
    p.write("src/main.mind", &src);
    let (code, err, art) = p.build_native("src/main.mind");

    assert_ne!(code, 0, "a lowering refusal must not succeed: {err}");
    assert!(
        art.is_none(),
        "a refused lowering must write NO artifact, never a partial one"
    );
    assert!(
        err.contains("lower.materialization_refused"),
        "the refusal must carry its stable kind so the branch is identifiable: {err}"
    );
    assert!(
        err.contains("error[backend-native]"),
        "it must surface as a native-backend diagnostic: {err}"
    );
    assert!(
        !err.contains("panicked"),
        "it must be a diagnostic, never an abort: {err}"
    );
}
