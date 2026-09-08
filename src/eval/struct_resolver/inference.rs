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

//! Declaration-driven record inference used by the field-access AST walk.

use std::collections::{HashMap, HashSet};

use crate::ast::{Literal, Node, TypeAnn};

use super::{FieldTypes, ResolverReturns};

/// Peel borrow / slice wrappers off a type annotation and return the
/// inner `Named` type name, if any. `&Cfg` / `&mut Cfg` / `[Cfg]` /
/// `&[Cfg]` all resolve to `Cfg`; a bare `Named(Cfg)` passes through.
///
/// This is what lets a by-reference struct parameter (`c: &Cfg`) or a
/// `-> &Cfg` return type seed the same field-access machinery that a
/// by-value `c: Cfg` parameter already used. It is purely additive:
/// non-`Ref`/`Slice` annotations that aren't `Named` still return
/// `None`, so no previously-emitted byte changes.
pub(super) fn unwrap_to_named(ty: &TypeAnn) -> Option<&str> {
    match ty {
        TypeAnn::Named(t) => Some(t.as_str()),
        TypeAnn::Ref { target, .. } => unwrap_to_named(target),
        TypeAnn::Slice { element, .. } => unwrap_to_named(element),
        _ => None,
    }
}

fn array_element_struct(ty: &TypeAnn, struct_defs: &[String]) -> Option<String> {
    let TypeAnn::Array { element, .. } = ty else {
        return None;
    };
    let name = unwrap_to_named(element)?;
    struct_defs
        .iter()
        .any(|known| known == name)
        .then(|| name.to_string())
}

/// Return the element name only for an exact declared fixed-array return.
pub(super) fn declared_array_element_named(ty: &TypeAnn) -> Option<&str> {
    let TypeAnn::Array { element, .. } = ty else {
        return None;
    };
    match element.as_ref() {
        TypeAnn::Named(name) => Some(name.as_str()),
        _ => None,
    }
}

pub(super) fn insert_unique_array_return(
    returns: &mut HashMap<String, TypeAnn>,
    ambiguous: &mut HashSet<String>,
    name: &str,
    schema: TypeAnn,
) {
    if ambiguous.contains(name) {
        return;
    }
    match returns.get(name) {
        Some(previous) if previous != &schema => {
            returns.remove(name);
            ambiguous.insert(name.to_string());
        }
        Some(_) => {}
        None => {
            returns.insert(name.to_string(), schema);
        }
    }
}

fn fixed_array_element_key(name: &str) -> String {
    // NUL cannot occur in a parsed identifier, so this internal side-table key
    // cannot alias a user binding.
    format!("\0fixed-array-element:{name}")
}

// A present scalar binding must stop Ident inference from falling back to a
// same-named module const. NUL cannot occur in source identifiers or struct
// names, so this value occupies the lexical slot without resolving as a type.
const NON_AGGREGATE_BINDING: &str = "\0non-aggregate-binding";

pub(super) fn update_binding(
    name: &str,
    ann: Option<&TypeAnn>,
    value: Option<&Node>,
    struct_defs: &[String],
    returns: &ResolverReturns,
    field_types: &FieldTypes,
    out: &mut HashMap<String, String>,
) {
    // Resolve the new value against the previous lexical binding first (`let a
    // = a` may intentionally read an outer `a`), then replace both side-table
    // entries together. A scalar shadow or reassignment must clear stale struct
    // and array-element knowledge instead of authorizing a later field load.
    let declared_struct = if value.is_none() {
        ann.and_then(unwrap_to_named)
            .filter(|candidate| struct_defs.iter().any(|known| known == *candidate))
            .map(str::to_string)
    } else {
        None
    };
    let direct = value
        .and_then(|node| infer_struct(node, out, returns, field_types))
        .or(declared_struct);
    let element = ann
        .and_then(|ty| array_element_struct(ty, struct_defs))
        .or_else(|| {
            value.and_then(|node| infer_array_element_struct(node, out, returns, field_types))
        });

    out.remove(name);
    out.remove(&fixed_array_element_key(name));
    out.insert(
        name.to_string(),
        direct
            .filter(|name| struct_defs.iter().any(|s| s == name))
            .unwrap_or_else(|| NON_AGGREGATE_BINDING.to_string()),
    );
    out.insert(
        fixed_array_element_key(name),
        element
            .filter(|name| struct_defs.iter().any(|s| s == name))
            .unwrap_or_else(|| NON_AGGREGATE_BINDING.to_string()),
    );
}

fn infer_array_element_struct(
    expr: &Node,
    vars: &HashMap<String, String>,
    returns: &ResolverReturns,
    field_types: &FieldTypes,
) -> Option<String> {
    match expr {
        Node::Lit(Literal::Ident(name), _) => vars
            .get(&fixed_array_element_key(name))
            .cloned()
            .or_else(|| {
                crate::ir::module_const_type(name).and_then(|ty| match ty {
                    TypeAnn::Array { element, .. } => unwrap_to_named(&element).map(str::to_string),
                    _ => None,
                })
            }),
        Node::ArrayLit { elements, .. } if !elements.is_empty() => {
            let first = infer_struct(&elements[0], vars, returns, field_types)?;
            elements
                .iter()
                .all(|item| {
                    infer_struct(item, vars, returns, field_types).as_deref() == Some(&first)
                })
                .then_some(first)
        }
        Node::Call { callee, .. } => returns
            .array_elements
            .get(callee)
            .and_then(|schema| declared_array_element_named(schema).map(str::to_owned)),
        Node::Paren(inner, _) | Node::Ref { inner, .. } => {
            infer_array_element_struct(inner, vars, returns, field_types)
        }
        _ => None,
    }
}

/// Infer the struct-type name of an expression, if known. Returns
/// `None` for scalar / unresolvable / not-a-struct expressions.
pub(super) fn infer_struct(
    expr: &Node,
    vars: &HashMap<String, String>,
    returns: &ResolverReturns,
    field_types: &FieldTypes,
) -> Option<String> {
    match expr {
        // The most common cases first — direct StructLit + Ident lookup.
        Node::StructLit { name, .. } => Some(name.clone()),
        Node::Lit(Literal::Ident(v), _) => vars.get(v).cloned().or_else(|| {
            crate::ir::module_const_type(v).and_then(|ty| unwrap_to_named(&ty).map(str::to_string))
        }),
        Node::Call { callee, .. } => returns.direct.get(callee).cloned(),
        Node::Paren(inner, _) => infer_struct(inner, vars, returns, field_types),
        Node::Ref { inner, .. } => infer_struct(inner, vars, returns, field_types),
        Node::IndexAccess { receiver, .. } => {
            infer_array_element_struct(receiver, vars, returns, field_types)
        }
        // Chained access — `a.b` resolves to the struct type of field `b`
        // when that field is itself struct-typed. We first resolve the
        // receiver `a` to its struct name `S` (recursively, so deeper
        // chains `a.b.c` work), then look up `(S, b)` in the per-field
        // type table built from the `StructDef` declarations. The table
        // already peeled `&T` / `[T]` wrappers and dropped scalar fields,
        // so a hit is always a known inner struct name and a scalar field
        // (`o.tag`) returns `None` exactly as before — keeping this arm
        // purely additive (nested struct-typed fields are absent from the
        // keystone and all of std, so no currently-emitted byte changes).
        Node::FieldAccess {
            receiver, field, ..
        } => {
            let recv_struct = infer_struct(receiver, vars, returns, field_types)?;
            field_types.get(&(recv_struct, field.clone())).cloned()
        }
        // BLOCKER 2 — UFCS method call bound to a `let`. `let s2 = s.grow(b)`
        // desugars in lowering to the free function `{lowercase(T)}_{method}`
        // (here `buf_grow`) with the receiver threaded first. For a later
        // `s2.field` access to resolve, `s2` must be recorded as the desugared
        // target's *return* struct type — exactly as the direct-call form
        // (`let s2 = buf_grow(s, b)`) already resolves through the `Node::Call`
        // arm above. We resolve the receiver's struct `T`, form the same
        // `{T.to_lowercase()}_{method}` name the lowering arm emits, and look
        // up its return type in the direct-return map. A zero-arg accessor that names a
        // field of `T` (`s.len()`) is a scalar field read, not a struct value,
        // so it correctly returns `None` (the UFCS target `{t}_len` will not be
        // a registered struct-returning free function).
        Node::MethodCall {
            receiver, method, ..
        } => {
            let recv_struct = infer_struct(receiver, vars, returns, field_types)?;
            let fn_name = format!("{}_{}", recv_struct.to_lowercase(), method);
            returns.direct.get(&fn_name).cloned()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array_schema(length: u32) -> TypeAnn {
        TypeAnn::Array {
            element: Box::new(TypeAnn::Named("Item".to_string())),
            length,
        }
    }

    #[test]
    fn array_return_schema_rejects_same_element_with_different_length() {
        let mut returns = HashMap::new();
        let mut ambiguous = HashSet::new();
        insert_unique_array_return(&mut returns, &mut ambiguous, "make_items", array_schema(1));
        insert_unique_array_return(&mut returns, &mut ambiguous, "make_items", array_schema(2));
        assert!(returns.is_empty());
        assert!(ambiguous.contains("make_items"));
        assert!(declared_array_element_named(&TypeAnn::ScalarI64).is_none());
    }
}
