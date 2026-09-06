// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Declared types and compatibility checks for collection-owner storage slots.

use std::collections::BTreeMap;

use crate::ast::{Node, TypeAnn};
use crate::diagnostics::Diagnostic;

use super::{COLLECTION_OWNER_ASSIGN_CODE, Env, provenance, report};

pub(crate) type StructFieldTypes = BTreeMap<(String, String), TypeAnn>;

fn global_struct_field_types() -> StructFieldTypes {
    let mut table = StructFieldTypes::new();
    crate::ir::with_global_enums(|global| {
        for (owner, (names, types)) in &global.structs {
            for (name, ty) in names.iter().zip(types) {
                table.insert((owner.clone(), name.clone()), ty.clone());
            }
        }
        for (owner, (names, types)) in &global.qualified_structs {
            for (name, ty) in names.iter().zip(types) {
                table.insert((owner.clone(), name.clone()), ty.clone());
            }
        }
    });
    table
}

fn collect_local_structs<'a>(
    items: &'a [Node],
    structs: &mut Vec<(&'a str, &'a [crate::ast::Field])>,
) {
    for item in items {
        match item {
            Node::StructDef { name, fields, .. } => structs.push((name, fields)),
            Node::Block { stmts, .. } => collect_local_structs(stmts, structs),
            _ => {}
        }
    }
}

pub(crate) fn struct_field_types(
    items: &[Node],
    inherited: Option<&std::rc::Rc<StructFieldTypes>>,
) -> std::rc::Rc<StructFieldTypes> {
    let mut local = Vec::new();
    collect_local_structs(items, &mut local);
    if local.is_empty() {
        return inherited
            .map(std::rc::Rc::clone)
            .unwrap_or_else(|| std::rc::Rc::new(global_struct_field_types()));
    }

    let mut table = inherited
        .map(|fields| fields.as_ref().clone())
        .unwrap_or_else(global_struct_field_types);
    for (owner, fields) in local {
        table.retain(|(known_owner, _), _| known_owner != owner);
        for field in fields {
            table.insert((owner.to_string(), field.name.clone()), field.ty.clone());
        }
    }
    std::rc::Rc::new(table)
}

fn display_type(ty: &TypeAnn) -> String {
    match ty {
        TypeAnn::ScalarI32 => "i32".to_string(),
        TypeAnn::ScalarI64 => "i64".to_string(),
        TypeAnn::ScalarU32 => "u32".to_string(),
        TypeAnn::ScalarF32 => "f32".to_string(),
        TypeAnn::ScalarF64 => "f64".to_string(),
        TypeAnn::ScalarBool => "bool".to_string(),
        TypeAnn::Named(name) => name.clone(),
        TypeAnn::Generic { name, args } => format!(
            "{name}<{}>",
            args.iter().map(display_type).collect::<Vec<_>>().join(", ")
        ),
        TypeAnn::Array { element, length } => format!("[{}; {length}]", display_type(element)),
        TypeAnn::Slice { mutable, element } => format!(
            "&{}[{}]",
            if *mutable { "mut " } else { "" },
            display_type(element)
        ),
        TypeAnn::Ref { mutable, target } => format!(
            "&{}{}",
            if *mutable { "mut " } else { "" },
            display_type(target)
        ),
        TypeAnn::Tuple { elements } => format!(
            "({})",
            elements
                .iter()
                .map(display_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => format!("{ty:?}"),
    }
}

pub(super) fn check(
    target: Option<TypeAnn>,
    value: &Node,
    context: &str,
    env: &Env,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Diagnostic>,
) {
    let Some(target) = target.filter(provenance::is_collection_owner_type) else {
        return;
    };
    if provenance::compatible_collection_owner_source(&target, value, env) {
        return;
    }
    let expected = display_type(&target);
    let actual = provenance::expr_type(value, env)
        .as_ref()
        .map(display_type)
        .map(|ty| format!("`{ty}`"))
        .unwrap_or_else(|| "a scalar, status, or unproven value".to_string());
    report(
        errs,
        src,
        file,
        format!(
            "assignment to {context} expects `{expected}`, but the right-hand side is {actual}; collection owners may only be replaced by a value of the same declared collection type"
        ),
        value.span(),
        COLLECTION_OWNER_ASSIGN_CODE,
    );
}
