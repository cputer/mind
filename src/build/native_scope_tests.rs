//! Seam tests for the captured-source snapshot and its module identities.
//!
//! Split out of `native_scope.rs` to keep that file under the 800-line ceiling
//! `tests/module_size_ratchet.rs` pins. Nothing was rewritten, minified or
//! dropped: every test and every assertion moved verbatim. It stays an in-crate
//! module (wired with `#[path]`) rather than an integration test because it
//! exercises `pub(crate) validate_captured` directly, which is the whole point
//! of testing at that seam.

use super::*;

/// The snapshot is SINGLE-SOURCE: the module table validation resolves
/// against and the bytes it checks both come from one `ProjectScope` capture.
///
/// This is a structural property, not a byte-divergence one -- in production
/// the two cannot disagree, because `sources` is built from
/// `scope.linked_sources()` and the table is that same scope's. What CAN
/// break it is swapping the adapter, so this test is written to detect
/// exactly that.
///
/// LIMIT OF THIS TEST, measured rather than assumed: it does NOT
/// discriminate `scope.install()` from `install_for_check`, and no
/// end-to-end fixture can. Two rounds of measurement:
///
/// 1. A flat `src/{main,helper}.mind` fixture fails to discriminate because
///    the reconstructed keys (`main`, `helper`) still satisfy the written
///    import `helper`.
/// 2. A fixture built specifically to defeat that -- `src/a/helper.mind`
///    and `src/b/helper.mind` sharing a last segment, so the full path is
///    required -- DOES discriminate the two rootings at the CLI:
///    `import src.a.helper` builds and runs, `import a.helper` refuses with
///    `scope.unresolved_import`. But patching `validate_captured` to
///    `install_for_check` and rebuilding leaves that fixture GREEN --
///    the mutation survives.
///
/// The reason is structural: import resolution never consults the installed
/// table. `resolve_native_sources` calls `scope.resolve_import_path`
/// directly on the scope object, upstream of this function, so the identity
/// decision is already settled before any guard exists. `validate_captured`
/// installs a table only to type-check the captured text; its guard governs
/// the TYPE/enum axis, not the import axis.
///
/// So `scope.install()` is correct by construction on the import axis --
/// resolution is the scope's own answer and cannot disagree with the bytes.
/// On the type axis it is still argued rather than gated. Gating it needs a
/// unit test at this function seam with a scope whose `install()` and
/// `install_for_check()` type tables provably differ; an end-to-end fixture
/// cannot reach it. Recorded as outstanding, not claimed.
#[test]
fn validation_resolves_through_the_captured_scope_identities() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"snap\"\nversion = \"0.1.0\"\n\n\
         [build]\ntarget = \"cpu\"\nentry = \"src/main.mind\"\n",
    )
    .unwrap();
    let helper = "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n                          return x + 1;\n}\n";
    std::fs::write(root.join("src/helper.mind"), helper).unwrap();
    let main_src = "import helper;\n\nfn main() -> i64 {\n    return public_fn(6);\n}\n";
    std::fs::write(root.join("src/main.mind"), main_src).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status();

    let entry = root.join("src/main.mind");
    let scope =
        match crate::project::single_file_scope::discover_with_source(&entry, "cpu", main_src) {
            Ok(crate::project::single_file_scope::Discovery::Project(s)) => s,
            Ok(crate::project::single_file_scope::Discovery::SingleTranslationUnit) => {
                panic!("fixture must resolve as a project, got SingleTranslationUnit")
            }
            Ok(crate::project::single_file_scope::Discovery::MissingProject(i)) => {
                panic!("fixture must resolve as a project, got MissingProject({i:?})")
            }
            Err(e) => panic!("discovery failed: {e:#}"),
        };

    // The captured set carries the scope's OWN canonical module identities.
    let captured: Vec<NativeSource> = scope
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope.entry(),
        })
        .collect();
    assert!(
        captured.iter().any(|s| s.module_path == "crate.helper"),
        "the capture must carry canonical identities; got {:?}",
        captured.iter().map(|s| &s.module_path).collect::<Vec<_>>()
    );

    // A legal cross-module call validates ONLY if resolution runs against the
    // capture's identities. Under a table rebuilt from file paths the callee
    // is not found and this refuses.
    validate_captured(&captured, &scope)
        .expect("a legal cross-module call must validate through the captured scope");
}

/// INTRA-BUILD snapshot: one `validate_captured` call, disk changed under it.
///
/// This is the control the independent review asked for. The three CLI
/// controls named "snapshot" in the integration test are NOT this: each of
/// them completes a build, mutates disk, then starts a WHOLLY SEPARATE second
/// invocation. An implementation that re-read disk inside every invocation
/// would pass all three, so they cannot witness the property they claim.
///
/// This one captures a scope and its bytes, mutates a DEFINING source on
/// disk, and then calls `validate_captured` with the PRE-mutation bytes and
/// the PRE-mutation scope. The verdict must follow the capture, not the disk.
///
/// The mutation is proven observable rather than assumed: after changing the
/// file, a FRESH discovery over the same tree is taken and validated, and
/// that must REFUSE. Without that positive control this test would pass just
/// as happily if the edit had silently done nothing.
#[test]
fn validation_follows_the_capture_when_disk_changes_underneath_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"snapintra\"\nversion = \"0.1.0\"\n\n\
         [build]\ntarget = \"cpu\"\nentry = \"src/main.mind\"\n",
    )
    .unwrap();
    let helper_before =
        "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n";
    std::fs::write(root.join("src/helper.mind"), helper_before).unwrap();
    let main_src = "import helper;\n\nfn main() -> i64 {\n    return public_fn(6);\n}\n";
    std::fs::write(root.join("src/main.mind"), main_src).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status();

    let entry = root.join("src/main.mind");
    let capture = |src: &str| match crate::project::single_file_scope::discover_with_source(
        &entry, "cpu", src,
    ) {
        Ok(crate::project::single_file_scope::Discovery::Project(s)) => s,
        other => panic!("fixture must resolve as a project: {:?}", other.is_ok()),
    };

    let scope_before = capture(main_src);
    let captured_before: Vec<NativeSource> = scope_before
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope_before.entry(),
        })
        .collect();
    validate_captured(&captured_before, &scope_before)
        .expect("the pre-mutation capture must validate");

    // Change the DEFINING source on disk: the callee no longer exists.
    std::fs::write(
        root.join("src/helper.mind"),
        "export { other_fn }\n\npub fn other_fn(x: i64) -> i64 {\n    return x + 2;\n}\n",
    )
    .unwrap();

    // POSITIVE CONTROL: the edit is real and this seam can see it. A fresh
    // discovery over the changed tree must now REFUSE through the same call.
    let scope_after = capture(main_src);
    let captured_after: Vec<NativeSource> = scope_after
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope_after.entry(),
        })
        .collect();
    assert!(
        validate_captured(&captured_after, &scope_after).is_err(),
        "positive control failed: after deleting the callee a FRESH capture must \
         refuse, otherwise this test proves nothing about snapshots"
    );

    // THE ASSERTION: same seam, same pre-mutation inputs, disk now different.
    // The verdict must follow the bytes that were captured.
    validate_captured(&captured_before, &scope_before).expect(
        "validation must follow the CAPTURE, not the disk; a re-read inside this \
         call would now refuse because the callee was removed after capture",
    );
}

/// IDENTITY axis: the captured scope's own module keys, not a rebuilt table.
///
/// The sibling snapshot test above gates the SNAPSHOT property but NOT this
/// one -- measured: swapping `scope.install()` for `install_for_check` leaves
/// it green, because for a flat `src/{main,helper}.mind` layout the
/// common-ancestor rooting still yields the key `helper`, which satisfies the
/// written `import helper`.
///
/// This fixture was built to defeat that: two modules share a last segment so
/// the FULL path is required, and the manifest declares its sources
/// explicitly so the scope roots at the project root (`src.a.helper`) while
/// `install_for_check` roots at the common ancestor `src/` (`a.helper`).
///
/// IT STILL DOES NOT DISCRIMINATE, and the reason is worth recording because
/// it means no fixture can. MEASURED, three ways:
///
/// 1. Swapping `scope.install()` for `install_for_check` leaves BOTH this
///    test and the sibling snapshot test green.
/// 2. An entry importing only `src.a.helper` but CALLING `from_b`, defined
///    in the un-imported sibling, passes `validate_captured` and is caught
///    later by closure admission (`closure.unresolved_edge`).
/// 3. So this seam's symbol lookup spans every module in the installed
///    table; it is not scoped by the import graph, and re-keying the modules
///    changes nothing it can observe.
///
/// `validate_captured` type-checks. Import scoping is enforced downstream by
/// `admit_native_closure`, and identity is settled upstream by
/// `scope.resolve_import_path`. The `scope.install()` choice is therefore
/// correct by construction at BOTH ends and unobservable in the middle --
/// stated as measured rather than gated, because claiming a mutation here
/// that does not exist would be worse than admitting the gap.
///
/// What this test does hold: the capture path resolves a path-qualified
/// identity end to end. It is a positive control, not a mutation gate.
#[test]
fn validation_uses_the_captured_identities_not_a_reconstructed_rooting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/a")).unwrap();
    std::fs::create_dir_all(root.join("src/b")).unwrap();
    std::fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"ident\"\nversion = \"0.1.0\"\n\n\
         [build]\ntarget = \"cpu\"\nentry = \"src/main.mind\"\n\n\
         [targets.cpu]\nbackend = \"native\"\n\
         sources = [\"src/main.mind\", \"src/a/helper.mind\", \"src/b/helper.mind\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/a/helper.mind"),
        "export { from_a }\n\npub fn from_a(x: i64) -> i64 {\n    return x + 1;\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/b/helper.mind"),
        "export { from_b }\n\npub fn from_b(x: i64) -> i64 {\n    return x + 100;\n}\n",
    )
    .unwrap();
    let main_src = "import src.a.helper;\n\nfn main() -> i64 {\n    return from_a(6);\n}\n";
    std::fs::write(root.join("src/main.mind"), main_src).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status();

    let entry = root.join("src/main.mind");
    let scope =
        match crate::project::single_file_scope::discover_with_source(&entry, "cpu", main_src) {
            Ok(crate::project::single_file_scope::Discovery::Project(s)) => s,
            other => panic!("fixture must resolve as a project: ok={}", other.is_ok()),
        };

    let captured: Vec<NativeSource> = scope
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope.entry(),
        })
        .collect();

    // The capture must actually carry the project-root rooting, or the
    // fixture is not testing what it claims.
    assert!(
        captured.iter().any(|s| s.module_path.contains("a.helper")),
        "fixture precondition: capture must carry a path-qualified identity, got {:?}",
        captured.iter().map(|s| &s.module_path).collect::<Vec<_>>()
    );

    validate_captured(&captured, &scope)
        .expect("the captured rooting resolves `src.a.helper`; a rebuilt table does not");
}

/// Negative twin: an unexported callee refuses through the same seam, so the
/// positive above is not simply "everything passes".
#[test]
fn validation_refuses_an_unexported_callee_through_the_same_seam() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"snapneg\"\nversion = \"0.1.0\"\n\n\
         [build]\ntarget = \"cpu\"\nentry = \"src/main.mind\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/helper.mind"),
        "export { public_fn }\n\npub fn public_fn(x: i64) -> i64 {\n    return x + 1;\n}\n\n\
         fn secret(x: i64) -> i64 {\n    return x + 100;\n}\n",
    )
    .unwrap();
    let main_src = "import helper;\n\nfn main() -> i64 {\n    return secret(5);\n}\n";
    std::fs::write(root.join("src/main.mind"), main_src).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status();

    let entry = root.join("src/main.mind");
    let scope =
        match crate::project::single_file_scope::discover_with_source(&entry, "cpu", main_src) {
            Ok(crate::project::single_file_scope::Discovery::Project(s)) => s,
            _ => panic!("fixture must resolve as a project"),
        };
    let captured: Vec<NativeSource> = scope
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope.entry(),
        })
        .collect();
    let err = validate_captured(&captured, &scope)
        .expect_err("an unexported callee must refuse through this seam");
    assert_eq!(err.kind, "scope.rejected_by_type_check");
}

/// Each captured source is type-checked under ITS OWN owner, not the entry's.
///
/// Owner-qualified resolution reads an ambient current module. Without a
/// per-source guard the whole set is checked under the entry's owner, and two
/// things go wrong at once: a sibling's imports resolve against the entry's
/// import graph, and a symbol visible only to the entry looks visible to every
/// module in the image.
///
/// POSITIVE: a legal two-module program, each module importing what it actually
/// needs, validates through the seam.
#[test]
fn each_captured_source_is_checked_under_its_own_owner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"owner\"\nversion = \"0.1.0\"\n\n\
         [build]\ntarget = \"cpu\"\nentry = \"src/main.mind\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/leaf.mind"),
        "export { leaf }\n\npub fn leaf(x: i64) -> i64 {\n    return x + 1;\n}\n",
    )
    .unwrap();
    // The sibling declares its OWN import of leaf. Under the entry's owner this
    // would still appear resolvable; under its own owner it must be, and is.
    std::fs::write(
        root.join("src/mid.mind"),
        "import leaf;\n\nexport { mid }\n\npub fn mid(x: i64) -> i64 {\n    return leaf(x) + 1;\n}\n",
    )
    .unwrap();
    let main_src = "import mid;\n\nfn main() -> i64 {\n    return mid(5);\n}\n";
    std::fs::write(root.join("src/main.mind"), main_src).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status();

    let entry = root.join("src/main.mind");
    let scope =
        match crate::project::single_file_scope::discover_with_source(&entry, "cpu", main_src) {
            Ok(crate::project::single_file_scope::Discovery::Project(s)) => s,
            other => panic!("fixture must resolve as a project: ok={}", other.is_ok()),
        };
    let captured: Vec<NativeSource> = scope
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope.entry(),
        })
        .collect();
    assert!(
        captured.len() >= 2,
        "fixture precondition: the closure must carry the sibling, got {:?}",
        captured.iter().map(|s| &s.module_path).collect::<Vec<_>>()
    );
    validate_captured(&captured, &scope).expect("each module validates under its own owner");
}

/// NEGATIVE, wrong owner: a sibling calls a symbol that only the ENTRY imports.
///
/// The sibling declares no import of `leaf`, so under its own owner `leaf` is
/// not visible and the set must be refused. If the entry's owner were installed
/// for every source, the entry's `import leaf` would make the call look legal
/// and the program would be admitted — that is exactly the wrong acceptance
/// this guard prevents.
#[test]
fn a_sibling_may_not_borrow_the_entry_owners_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Mind.toml"),
        "[package]\nname = \"wrongowner\"\nversion = \"0.1.0\"\n\n\
         [build]\ntarget = \"cpu\"\nentry = \"src/main.mind\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/leaf.mind"),
        "export { leaf }\n\npub fn leaf(x: i64) -> i64 {\n    return x + 1;\n}\n",
    )
    .unwrap();
    // NO `import leaf;` here, yet it calls leaf.
    std::fs::write(
        root.join("src/mid.mind"),
        "export { mid }\n\npub fn mid(x: i64) -> i64 {\n    return leaf(x) + 1;\n}\n",
    )
    .unwrap();
    // The ENTRY imports both, so under the entry's owner `leaf` is visible.
    let main_src = "import leaf;\nimport mid;\n\nfn main() -> i64 {\n    return mid(5);\n}\n";
    std::fs::write(root.join("src/main.mind"), main_src).unwrap();
    let _ = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status();

    let entry = root.join("src/main.mind");
    let scope =
        match crate::project::single_file_scope::discover_with_source(&entry, "cpu", main_src) {
            Ok(crate::project::single_file_scope::Discovery::Project(s)) => s,
            other => panic!("fixture must resolve as a project: ok={}", other.is_ok()),
        };
    let captured: Vec<NativeSource> = scope
        .linked_sources()
        .map(|c| NativeSource {
            module_path: c.module_path().to_string(),
            path: c.path().to_path_buf(),
            source: c.source().as_bytes().to_vec(),
            is_entry: c.path() == scope.entry(),
        })
        .collect();
    assert!(
        captured.iter().any(|s| s.module_path.ends_with("mid")),
        "fixture precondition: the borrowing sibling must be captured"
    );
    assert!(
        validate_captured(&captured, &scope).is_err(),
        "a sibling that calls a symbol only the ENTRY imports must be refused; \
         admitting it means every source was checked under the entry's owner"
    );
}
