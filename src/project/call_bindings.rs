// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Owner-preserving source-function resolution.
//!
//! Existing checker lookups return an ownerless signature or `None`, where
//! `None` combines absence with ambiguity. Canonical lowering needs a stronger
//! answer before it can attach function and call identities. This module keeps
//! that answer separate and inert: no existing checker, lowering, wire, or
//! backend path calls it yet.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{Module, Node, TypeAnn};
use crate::types::FunctionIdentity;

use super::module_table::{ExportedFn, ModuleTable};

/// A source function spelling at one already parsed call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionReference<'a> {
    Bare(&'a str),
    /// `import_path` is the qualifier recorded by the parser, before project
    /// discovery maps a short or aliased spelling to its canonical owner.
    Qualified {
        import_path: &'a [String],
        name: &'a str,
    },
}

/// A declaration signature together with the owner in whose type namespace
/// every remaining `TypeAnn::Named` must be interpreted.
///
/// Resolution is valid only with the same captured [`ProjectScope`] that
/// produced this value. An unqualified remaining name belongs to
/// `defining_owner`; this slice refuses qualified names because resolving one
/// also requires that owner's captured import/type environment.
///
/// [`ProjectScope`]: super::single_file_scope::ProjectScope
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefiningSignature {
    defining_owner: String,
    param_types: Vec<TypeAnn>,
    return_type: Option<TypeAnn>,
}

impl DefiningSignature {
    pub fn defining_owner(&self) -> &str {
        &self.defining_owner
    }

    pub fn param_types(&self) -> &[TypeAnn] {
        &self.param_types
    }

    pub fn return_type(&self) -> Option<&TypeAnn> {
        self.return_type.as_ref()
    }
}

/// One uniquely owned source declaration. Source functions are deliberately
/// not classified as external or intrinsic here; those authorities require
/// explicit registries in later slices and must never be guessed from names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCandidate {
    identity: FunctionIdentity,
    signature: DefiningSignature,
}

/// Owner-qualified spelling of a source declaration before the resolver has
/// proved that it can construct a canonical function identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceFunctionDeclaration {
    owner: String,
    name: String,
}

impl SourceFunctionDeclaration {
    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Why a uniquely authoritative source declaration cannot yet become a
/// canonical lowering candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedFunctionReason {
    /// A generic declaration requires call-site substitution before an
    /// identity with concrete `type_args` exists.
    GenericDeclaration { type_params: Box<[String]> },
    /// This annotation needs the defining module's captured import/type
    /// environment; an owner label alone cannot resolve it soundly.
    QualifiedTypeAnnotation { name: String },
}

/// One found, owner-qualified declaration that this source-only resolver
/// deliberately cannot turn into a canonical function identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedFunction {
    declaration: SourceFunctionDeclaration,
    reason: UnsupportedFunctionReason,
}

impl UnsupportedFunction {
    pub fn declaration(&self) -> &SourceFunctionDeclaration {
        &self.declaration
    }

    pub fn reason(&self) -> &UnsupportedFunctionReason {
        &self.reason
    }
}

impl FunctionCandidate {
    pub fn identity(&self) -> &FunctionIdentity {
        &self.identity
    }

    pub fn signature(&self) -> &DefiningSignature {
        &self.signature
    }
}

/// A total source-function lookup result. Ambiguity retains sorted
/// owner-qualified declarations even when every candidate has the same
/// structural signature or one candidate is not yet a supported capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionResolution {
    Unique(FunctionCandidate),
    Ambiguous(Box<[SourceFunctionDeclaration]>),
    Unsupported(UnsupportedFunction),
    Missing,
}

/// Resolve the parser's import spelling through the same captured binding and
/// exact module-table fallback used by [`super::single_file_scope::ProjectScope`].
pub(crate) fn resolve_import_target<'a>(
    table: &'a ModuleTable,
    resolved_imports: &'a BTreeMap<(String, Vec<String>), String>,
    source_owner: &str,
    import_path: &[String],
) -> Option<&'a str> {
    resolved_imports
        .get(&(source_owner.to_string(), import_path.to_vec()))
        .map(String::as_str)
        .or_else(|| {
            table
                .get_import(import_path)
                .map(|module| module.module_path.as_str())
        })
}

/// Resolve one source call against its defining module and the exact imports
/// captured for that source owner.
///
/// Standard-library and intrinsic calls are outside this source-`FnDef`
/// authority. They remain [`FunctionResolution::Missing`] until a separate
/// authoritative resolver classifies them; their identity is never inferred
/// from a callee spelling.
pub(crate) fn resolve_function(
    table: &ModuleTable,
    resolved_imports: &BTreeMap<(String, Vec<String>), String>,
    source_owner: &str,
    source_module: Option<&Module>,
    reference: FunctionReference<'_>,
) -> FunctionResolution {
    match reference {
        FunctionReference::Bare(name) => {
            // Lexical source declarations are authoritative before imported
            // bare names. This includes private functions omitted from the
            // exported surface.
            let local = source_module
                .map(|module| local_candidates(source_owner, module, name))
                .unwrap_or_default();
            if !local.is_empty() {
                return classify(local);
            }
            let targets: BTreeSet<&str> = resolved_imports
                .iter()
                .filter(|((importer, _), _)| importer == source_owner)
                .map(|(_, target)| target.as_str())
                .filter(|target| !is_std_owner(target))
                .collect();
            let imported = targets
                .into_iter()
                .flat_map(|owner| exported_candidates(table, owner, name))
                .collect();
            classify(imported)
        }
        FunctionReference::Qualified { import_path, name } => {
            let Some(owner) = resolve_qualified_target(
                table,
                resolved_imports,
                source_owner,
                source_module,
                import_path,
            ) else {
                return FunctionResolution::Missing;
            };
            if is_std_owner(owner) {
                return FunctionResolution::Missing;
            }
            if owner == source_owner {
                return classify(
                    source_module
                        .map(|module| local_candidates(source_owner, module, name))
                        .unwrap_or_default(),
                );
            }
            classify(exported_candidates(table, owner, name))
        }
    }
}

fn is_std_owner(owner: &str) -> bool {
    owner == "std" || owner.starts_with("std.")
}

fn resolve_qualified_target<'a>(
    table: &'a ModuleTable,
    resolved_imports: &'a BTreeMap<(String, Vec<String>), String>,
    source_owner: &str,
    source_module: Option<&Module>,
    qualifier: &[String],
) -> Option<&'a str> {
    // A full qualifier may itself be the captured import path. Prefer that
    // exact binding before considering the parser's short, last-segment form.
    if let Some(target) = resolved_imports.get(&(source_owner.to_string(), qualifier.to_vec())) {
        return Some(target);
    }

    fn collect<'a>(items: &'a [Node], qualifier: &[String], out: &mut Vec<&'a [String]>) {
        for item in items {
            match item {
                Node::Import { path, .. }
                    if path == qualifier
                        || (qualifier.len() == 1 && path.last() == qualifier.first()) =>
                {
                    out.push(path)
                }
                Node::Block { stmts, .. } => collect(stmts, qualifier, out),
                _ => {}
            }
        }
    }

    let mut matching_paths = Vec::new();
    if let Some(module) = source_module {
        collect(&module.items, qualifier, &mut matching_paths);
    }
    if matching_paths.len() == 1 {
        return resolve_import_target(table, resolved_imports, source_owner, matching_paths[0]);
    }
    // The project capture is the visibility authority. A module-table entry
    // that the source did not import must not become reachable merely because
    // the caller supplied its canonical spelling.
    None
}

#[derive(Debug)]
struct DeclarationCandidate {
    declaration: SourceFunctionDeclaration,
    type_params: Vec<String>,
    signature: DefiningSignature,
}

fn classify(mut candidates: Vec<DeclarationCandidate>) -> FunctionResolution {
    candidates.sort_by(|a, b| a.declaration.cmp(&b.declaration));
    match candidates.len() {
        0 => FunctionResolution::Missing,
        1 => classify_unique(candidates.pop().expect("one candidate")),
        _ => FunctionResolution::Ambiguous(
            candidates
                .into_iter()
                .map(|candidate| candidate.declaration)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn classify_unique(candidate: DeclarationCandidate) -> FunctionResolution {
    if !candidate.type_params.is_empty() {
        return FunctionResolution::Unsupported(UnsupportedFunction {
            declaration: candidate.declaration,
            reason: UnsupportedFunctionReason::GenericDeclaration {
                type_params: candidate.type_params.into_boxed_slice(),
            },
        });
    }
    if let Some(name) = signature_qualified_name(&candidate.signature) {
        return FunctionResolution::Unsupported(UnsupportedFunction {
            declaration: candidate.declaration,
            reason: UnsupportedFunctionReason::QualifiedTypeAnnotation { name },
        });
    }
    FunctionResolution::Unique(FunctionCandidate {
        identity: FunctionIdentity::new(&candidate.declaration.owner, &candidate.declaration.name),
        signature: candidate.signature,
    })
}

fn signature_qualified_name(signature: &DefiningSignature) -> Option<String> {
    signature
        .param_types
        .iter()
        .find_map(qualified_name)
        .or_else(|| signature.return_type.as_ref().and_then(qualified_name))
        .map(str::to_string)
}

fn qualified_name(ty: &TypeAnn) -> Option<&str> {
    match ty {
        TypeAnn::Named(name) if name.contains('.') => Some(name),
        TypeAnn::Slice { element, .. }
        | TypeAnn::Array { element, .. }
        | TypeAnn::SparseTensor { element, .. } => qualified_name(element),
        TypeAnn::Ref { target, .. }
        | TypeAnn::RawPtr {
            pointee: target, ..
        } => qualified_name(target),
        TypeAnn::Generic { name, args } => name
            .contains('.')
            .then_some(name.as_str())
            .or_else(|| args.iter().find_map(qualified_name)),
        TypeAnn::Tuple { elements } => elements.iter().find_map(qualified_name),
        TypeAnn::FnPtr { params, ret } => params
            .iter()
            .find_map(qualified_name)
            .or_else(|| ret.as_deref().and_then(qualified_name)),
        TypeAnn::ScalarI32
        | TypeAnn::ScalarI64
        | TypeAnn::ScalarF32
        | TypeAnn::ScalarF64
        | TypeAnn::ScalarBool
        | TypeAnn::ScalarU32
        | TypeAnn::Tensor { .. }
        | TypeAnn::DiffTensor { .. }
        | TypeAnn::Named(_) => None,
    }
}

fn exported_candidates(table: &ModuleTable, owner: &str, name: &str) -> Vec<DeclarationCandidate> {
    table
        .get(owner)
        .into_iter()
        .flat_map(|module| module.exported_fns.iter())
        .filter(|function| function.name == name)
        .map(|function| candidate(owner, function))
        .collect()
}

fn local_candidates(owner: &str, module: &Module, name: &str) -> Vec<DeclarationCandidate> {
    fn collect<'a>(items: &'a [Node], name: &str, out: &mut Vec<&'a crate::ast::FnDefData>) {
        for item in items {
            match item {
                Node::FnDef(function, _) if function.name == name => out.push(function),
                Node::Block { stmts, .. } => collect(stmts, name, out),
                _ => {}
            }
        }
    }

    let aliases = crate::eval::type_aliases::LocalTypeAliases::new(&module.items);
    let mut functions = Vec::new();
    collect(&module.items, name, &mut functions);
    functions
        .into_iter()
        .map(|function| {
            candidate(
                owner,
                &ExportedFn {
                    name: function.name.clone(),
                    type_params: function.type_params.clone(),
                    param_types: function
                        .params
                        .iter()
                        .map(|param| aliases.resolve(&param.ty))
                        .collect(),
                    ret_type: function.ret_type.as_ref().map(|ty| aliases.resolve(ty)),
                },
            )
        })
        .collect()
}

fn candidate(owner: &str, function: &ExportedFn) -> DeclarationCandidate {
    DeclarationCandidate {
        declaration: SourceFunctionDeclaration {
            owner: owner.to_string(),
            name: function.name.clone(),
        },
        type_params: function.type_params.clone(),
        signature: DefiningSignature {
            defining_owner: owner.to_string(),
            param_types: function.param_types.clone(),
            return_type: function.ret_type.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Module {
        crate::parser::parse(source).expect("test source parses")
    }

    fn table(modules: &[(String, Module)]) -> ModuleTable {
        let refs = modules
            .iter()
            .map(|(owner, module)| (owner.clone(), module))
            .collect::<Vec<_>>();
        super::super::module_table::build_module_table(&refs)
    }

    fn imported(
        importer: &str,
        imports: &[(&[&str], &str)],
    ) -> BTreeMap<(String, Vec<String>), String> {
        imports
            .iter()
            .map(|(path, owner)| {
                (
                    (
                        importer.to_string(),
                        path.iter().map(|part| (*part).to_string()).collect(),
                    ),
                    (*owner).to_string(),
                )
            })
            .collect()
    }

    fn declaration(owner: &str, name: &str) -> SourceFunctionDeclaration {
        SourceFunctionDeclaration {
            owner: owner.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn equal_signatures_from_two_imported_owners_are_ambiguous_and_sorted() {
        let modules = [
            ("crate.z".into(), parse("pub fn same(x: i64) -> i64 { x }")),
            ("crate.a".into(), parse("pub fn same(x: i64) -> i64 { x }")),
        ];
        let table = table(&modules);
        let imports = imported(
            "crate.consumer",
            &[(&["z"], "crate.z"), (&["a"], "crate.a")],
        );
        let result = resolve_function(
            &table,
            &imports,
            "crate.consumer",
            None,
            FunctionReference::Bare("same"),
        );
        let FunctionResolution::Ambiguous(owners) = result else {
            panic!("same-signature owners must remain ambiguous")
        };
        assert_eq!(
            owners.as_ref(),
            [
                declaration("crate.a", "same"),
                declaration("crate.z", "same"),
            ]
        );
    }

    #[test]
    fn qualified_short_import_resolves_the_captured_exact_owner() {
        let modules = [
            (
                "crate.deep.tools".into(),
                parse("pub fn make() -> i64 { 7 }"),
            ),
            (
                "crate.other.tools".into(),
                parse("pub fn make() -> i64 { 9 }"),
            ),
        ];
        let table = table(&modules);
        let imports = imported(
            "crate.consumer",
            &[(&["crate", "deep", "tools"], "crate.deep.tools")],
        );
        let local = parse("import crate.deep.tools\nfn make() -> i64 { 11 }");
        let path = vec!["tools".to_string()];
        let result = resolve_function(
            &table,
            &imports,
            "crate.consumer",
            Some(&local),
            FunctionReference::Qualified {
                import_path: &path,
                name: "make",
            },
        );
        let FunctionResolution::Unique(target) = result else {
            panic!("captured short import must select one exact owner")
        };
        assert_eq!(
            target.identity(),
            &FunctionIdentity::new("crate.deep.tools", "make")
        );
    }

    #[test]
    fn hidden_and_unknown_qualified_functions_are_missing() {
        let modules = [(
            "crate.lib".into(),
            parse("export { visible }\nfn visible() -> i64 { 1 }\nfn hidden() -> i64 { 2 }"),
        )];
        let table = table(&modules);
        let imports = imported("crate.consumer", &[(&["lib"], "crate.lib")]);
        let path = vec!["lib".to_string()];
        for name in ["hidden", "unknown"] {
            assert_eq!(
                resolve_function(
                    &table,
                    &imports,
                    "crate.consumer",
                    None,
                    FunctionReference::Qualified {
                        import_path: &path,
                        name,
                    },
                ),
                FunctionResolution::Missing
            );
        }

        let unimported_path = vec!["crate".to_string(), "lib".to_string()];
        assert_eq!(
            resolve_function(
                &table,
                &BTreeMap::new(),
                "crate.consumer",
                Some(&parse("fn local() -> i64 { 0 }")),
                FunctionReference::Qualified {
                    import_path: &unimported_path,
                    name: "visible",
                },
            ),
            FunctionResolution::Missing
        );
    }

    #[test]
    fn source_local_declaration_precedes_an_imported_same_name() {
        let local = parse("export { other }\nfn same() -> i64 { 3 }\nfn other() -> i64 { 0 }");
        let modules = [("crate.a".into(), parse("pub fn same() -> i64 { 7 }"))];
        let table = table(&modules);
        let imports = imported("crate.consumer", &[(&["a"], "crate.a")]);
        let result = resolve_function(
            &table,
            &imports,
            "crate.consumer",
            Some(&local),
            FunctionReference::Bare("same"),
        );
        let FunctionResolution::Unique(target) = result else {
            panic!("local declaration must have lexical precedence")
        };
        assert_eq!(
            target.identity(),
            &FunctionIdentity::new("crate.consumer", "same")
        );
    }

    #[test]
    fn signature_named_types_retain_their_defining_owner() {
        let source = |value| {
            parse(&format!(
                "struct Item {{ value: i64 }}\n\
                 type Items = [Item; 1]\n\
                 pub fn make() -> Items {{ [Item {{ value: {value} }}] }}"
            ))
        };
        let modules = [("crate.a".into(), source(1)), ("crate.b".into(), source(2))];
        let table = table(&modules);
        let imports = imported(
            "crate.consumer",
            &[(&["a"], "crate.a"), (&["b"], "crate.b")],
        );

        for (path_part, owner) in [("a", "crate.a"), ("b", "crate.b")] {
            let path = vec![path_part.to_string()];
            let FunctionResolution::Unique(target) = resolve_function(
                &table,
                &imports,
                "crate.consumer",
                None,
                FunctionReference::Qualified {
                    import_path: &path,
                    name: "make",
                },
            ) else {
                panic!("qualified function must resolve")
            };
            assert_eq!(target.signature().defining_owner(), owner);
            assert_eq!(target.signature().param_types(), []);
            assert_eq!(
                target.signature().return_type(),
                Some(&TypeAnn::Array {
                    element: Box::new(TypeAnn::Named("Item".to_string())),
                    length: 1,
                })
            );
        }
    }
}
