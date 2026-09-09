// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

#![cfg(feature = "cross-module-imports")]

use libmind::ast::TypeAnn;
use libmind::project::call_bindings::{
    FunctionReference, FunctionResolution, SourceFunctionDeclaration, UnsupportedFunctionReason,
};
use libmind::project::single_file_scope::{ProjectScope, capture_project_scope};
use libmind::types::FunctionIdentity;

fn project_scope(entry_source: &str, siblings: &[(&str, &str)]) -> ProjectScope {
    let root = tempfile::tempdir().expect("temporary project root");
    let entry = root.path().join("main.mind");
    std::fs::write(&entry, entry_source).expect("write entry source");
    let mut candidates = vec![entry.clone()];
    for (name, source) in siblings {
        let path = root.path().join(name);
        std::fs::write(&path, source).expect("write sibling source");
        candidates.push(path);
    }
    capture_project_scope(&entry, entry_source, &candidates, root.path())
        .expect("capture project scope")
}

fn declaration_tuple(declaration: &SourceFunctionDeclaration) -> (&str, &str) {
    (declaration.owner(), declaration.name())
}

#[test]
fn local_generic_declaration_blocks_imported_monomorphic_fallback() {
    let scope = project_scope(
        "import support\nfn same<T>(x: T) -> T { x }\n",
        &[("support.mind", "pub fn same(x: i64) -> i64 { x }\n")],
    );
    let FunctionResolution::Unsupported(function) =
        scope.resolve_function("crate", FunctionReference::Bare("same"))
    else {
        panic!("local generic declaration must retain lexical authority")
    };
    assert_eq!(declaration_tuple(function.declaration()), ("crate", "same"));
    assert_eq!(
        function.reason(),
        &UnsupportedFunctionReason::GenericDeclaration {
            type_params: vec!["T".to_string()].into_boxed_slice(),
        }
    );
}

#[test]
fn imported_generic_is_unsupported_and_cannot_disappear_from_ambiguity() {
    let generic_only = project_scope(
        "import generic\n",
        &[(
            "generic.mind",
            "export { choose }\nfn choose<T>(value: T) -> T { value }\n",
        )],
    );
    let FunctionResolution::Unsupported(function) =
        generic_only.resolve_function("crate", FunctionReference::Bare("choose"))
    else {
        panic!("imported generic declaration must be an explicit refusal")
    };
    assert_eq!(
        declaration_tuple(function.declaration()),
        ("crate.generic", "choose")
    );

    let mixed = project_scope(
        "import generic\nimport concrete\n",
        &[
            (
                "generic.mind",
                "pub fn choose<T>(value: T) -> T { value }\n",
            ),
            (
                "concrete.mind",
                "pub fn choose(value: i64) -> i64 { value }\n",
            ),
        ],
    );
    let FunctionResolution::Ambiguous(declarations) =
        mixed.resolve_function("crate", FunctionReference::Bare("choose"))
    else {
        panic!("an unsupported import must still participate in authority")
    };
    assert_eq!(
        declarations
            .iter()
            .map(declaration_tuple)
            .collect::<Vec<_>>(),
        [("crate.concrete", "choose"), ("crate.generic", "choose")]
    );
}

#[test]
fn qualified_signature_type_is_refused_until_defining_imports_are_resolved() {
    let scope = project_scope(
        "import api\n",
        &[
            ("dep.mind", "type Items = [i64; 1]\n"),
            (
                "api.mind",
                "import dep\npub fn count(items: dep.Items) -> i64 { 1 }\n",
            ),
        ],
    );
    let path = vec!["api".to_string()];
    let FunctionResolution::Unsupported(function) = scope.resolve_function(
        "crate",
        FunctionReference::Qualified {
            import_path: &path,
            name: "count",
        },
    ) else {
        panic!("qualified annotation must not masquerade as canonical")
    };
    assert_eq!(
        declaration_tuple(function.declaration()),
        ("crate.api", "count")
    );
    assert_eq!(
        function.reason(),
        &UnsupportedFunctionReason::QualifiedTypeAnnotation {
            name: "dep.Items".to_string(),
        }
    );
}

#[test]
fn local_alias_to_local_record_retains_the_defining_owner() {
    let scope = project_scope(
        "struct Item { value: i64 }\n\
         type LocalItem = Item\n\
         fn inspect(item: LocalItem) -> LocalItem { item }\n",
        &[],
    );
    let FunctionResolution::Unique(function) =
        scope.resolve_function("crate", FunctionReference::Bare("inspect"))
    else {
        panic!("owner-local alias resolution must stay supported")
    };
    assert_eq!(
        function.identity(),
        &FunctionIdentity::new("crate", "inspect")
    );
    assert_eq!(function.signature().defining_owner(), "crate");
    assert_eq!(
        function.signature().param_types(),
        [TypeAnn::Named("Item".to_string())]
    );
    assert_eq!(
        function.signature().return_type(),
        Some(&TypeAnn::Named("Item".to_string()))
    );
}

#[test]
fn std_and_intrinsic_spellings_are_outside_source_function_authority() {
    let scope = project_scope("import std.vec\n", &[]);
    for name in ["vec_new", "__mind_alloc"] {
        assert_eq!(
            scope.resolve_function("crate", FunctionReference::Bare(name)),
            FunctionResolution::Missing,
            "{name} requires the separate std/intrinsic resolver"
        );
    }
    let std_vec = vec!["std".to_string(), "vec".to_string()];
    assert_eq!(
        scope.resolve_function(
            "crate",
            FunctionReference::Qualified {
                import_path: &std_vec,
                name: "vec_new",
            },
        ),
        FunctionResolution::Missing
    );
}
