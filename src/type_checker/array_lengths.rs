// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Literal cardinality and literal element classes are known without
//! approximating a binding's value type. Check them outside the function-body
//! pass that deliberately filters generic type errors from its approximate env.
//! Schema-owned fixed-array fields use the same narrow pass, so their declared
//! `[i64; N]` contract is checked before lowering while opaque struct handles
//! remain opaque everywhere else.

use super::Pretty;

#[path = "array_control_flow.rs"]
mod array_control_flow;

use crate::ast::{Literal, Module, Node, TypeAnn};
use std::collections::BTreeMap;

pub(super) fn check(module: &Module, src: &str, file: Option<&str>, errors: &mut Vec<Pretty>) {
    let aliases = crate::eval::type_aliases::LocalTypeAliases::new(&module.items);
    for item in &module.items {
        super::nerve_walk::walk(item, &mut |node| match node {
            Node::Let {
                name,
                ann: Some(annotation),
                value,
                ..
            }
            | Node::Const {
                name,
                ty: Some(annotation),
                value,
                ..
            } => {
                check_literal(name, &aliases.resolve(annotation), value, src, file, errors);
            }
            Node::FnDef(fd, _) => check_fn_returns(fd, &aliases, src, file, errors),
            _ => {}
        });
    }

    // Struct literals are checked in a separate scoped pass because the normal
    // loose ValueType inference intentionally represents every struct as an
    // opaque i64 handle.  That representation is sufficient for ordinary
    // calls, but it cannot validate a schema-owned fixed-array field value.
    let mut fields = StructFieldChecker {
        aliases: &aliases,
        schema_scopes: Vec::new(),
        return_scopes: Vec::new(),
        src,
        file,
        errors,
    };
    let mut env = BTreeMap::new();
    fields.visit_sequence(&module.items, &mut env);
}

type StructSchema = BTreeMap<String, TypeAnn>;
type SchemaScope = BTreeMap<String, Option<StructSchema>>;
type ReturnScope = BTreeMap<String, Option<TypeAnn>>;

fn direct_struct_schemas(
    nodes: &[Node],
    aliases: &crate::eval::type_aliases::LocalTypeAliases,
) -> SchemaScope {
    let mut scope = BTreeMap::new();
    for node in nodes {
        let Node::StructDef { name, fields, .. } = node else {
            continue;
        };
        let schema = fields
            .iter()
            .map(|field| (field.name.clone(), aliases.resolve(&field.ty)))
            .collect();
        if scope.insert(name.clone(), Some(schema)).is_some() {
            // Duplicate declarations are diagnosed elsewhere. They cannot
            // provide a unique schema fact for this narrow consistency pass.
            scope.insert(name.clone(), None);
        }
    }
    scope
}

fn direct_function_returns(
    nodes: &[Node],
    aliases: &crate::eval::type_aliases::LocalTypeAliases,
) -> ReturnScope {
    let mut scope = BTreeMap::new();
    for node in nodes {
        let Node::FnDef(fd, _) = node else {
            continue;
        };
        // Generic return annotations need call-site substitution. This pass
        // has no such authority, so retain the lexical shadow but no type fact.
        let ret = fd
            .type_params
            .is_empty()
            .then(|| fd.ret_type.as_ref().map(|ty| aliases.resolve(ty)))
            .flatten();
        if scope.insert(fd.name.clone(), ret).is_some() {
            // Never use a same-named outer declaration to prove a duplicate.
            scope.insert(fd.name.clone(), None);
        }
    }
    scope
}

/// The expression facts needed by the schema-owned fixed-array check. This is
/// deliberately narrower than `ValueType`: only an exact fact may prove a
/// mismatch, while an unknown expression is deferred to authoritative type
/// checking and lowering admission.
struct StructFieldChecker<'a> {
    aliases: &'a crate::eval::type_aliases::LocalTypeAliases,
    schema_scopes: Vec<SchemaScope>,
    return_scopes: Vec<ReturnScope>,
    src: &'a str,
    file: Option<&'a str>,
    errors: &'a mut Vec<Pretty>,
}

impl StructFieldChecker<'_> {
    fn visit_stmt(&mut self, node: &Node, env: &mut BTreeMap<String, TypeAnn>) {
        match node {
            Node::FnDef(fd, _) => {
                // Functions see module bindings and nested functions capture
                // their enclosing lexical bindings. Parameters shadow both.
                let mut fn_env = env.clone();
                for param in &fd.params {
                    fn_env.insert(param.name.clone(), self.aliases.resolve(&param.ty));
                }
                self.visit_sequence(&fd.body, &mut fn_env);
            }
            Node::Let {
                name, ann, value, ..
            } => {
                self.visit_expr(value, env);
                let ty = ann
                    .as_ref()
                    .map(|annotation| self.aliases.resolve(annotation))
                    .or_else(|| self.expr_type(value, env));
                if let Some(ty) = ty {
                    env.insert(name.clone(), ty);
                } else {
                    env.remove(name);
                }
            }
            Node::Const {
                name, ty, value, ..
            } => {
                self.visit_expr(value, env);
                let value_ty = ty
                    .as_ref()
                    .map(|annotation| self.aliases.resolve(annotation))
                    .or_else(|| self.expr_type(value, env));
                if let Some(value_ty) = value_ty {
                    env.insert(name.clone(), value_ty);
                } else {
                    env.remove(name);
                }
            }
            Node::Assign { name, value, .. } => {
                self.visit_expr(value, env);
                if let Some(ty) = self.expr_type(value, env) {
                    env.insert(name.clone(), ty);
                } else {
                    env.remove(name);
                }
            }
            Node::LetTuple { names, value, .. } => {
                self.visit_expr(value, env);
                for name in names {
                    env.remove(name);
                }
            }
            Node::Return { value, .. } => {
                if let Some(value) = value {
                    self.visit_expr(value, env);
                }
            }
            Node::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.visit_expr(cond, env);
                let mut then_env = env.clone();
                self.visit_sequence(then_branch, &mut then_env);
                if let Some(else_branch) = else_branch {
                    let mut else_env = env.clone();
                    self.visit_sequence(else_branch, &mut else_env);
                }
            }
            Node::For {
                var,
                start,
                end,
                body,
                ..
            } => {
                self.visit_expr(start, env);
                self.visit_expr(end, env);
                let mut loop_env = env.clone();
                // Match the established checker contract: range induction
                // variables are i32 unless explicitly cast by the source.
                loop_env.insert(var.clone(), TypeAnn::ScalarI32);
                self.visit_sequence(body, &mut loop_env);
            }
            Node::ForEach {
                var,
                collection,
                body,
                ..
            } => {
                self.visit_expr(collection, env);
                let mut loop_env = env.clone();
                // The foreach ABI exposes each element as an i64 handle/value.
                loop_env.insert(var.clone(), TypeAnn::ScalarI64);
                self.visit_sequence(body, &mut loop_env);
            }
            #[cfg(feature = "std-surface")]
            Node::While { cond, body, .. } => {
                self.visit_expr(cond, env);
                let mut loop_env = env.clone();
                self.visit_sequence(body, &mut loop_env);
            }
            Node::Block { stmts, .. } => {
                let mut block_env = env.clone();
                self.visit_sequence(stmts, &mut block_env);
            }
            Node::Match {
                scrutinee, arms, ..
            } => {
                self.visit_expr(scrutinee, env);
                for arm in arms {
                    let mut arm_env = env.clone();
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(guard, &mut arm_env);
                    }
                    self.visit_expr(&arm.body, &mut arm_env);
                }
            }
            _ => self.visit_expr(node, env),
        }
    }

    fn visit_sequence(&mut self, nodes: &[Node], env: &mut BTreeMap<String, TypeAnn>) {
        self.schema_scopes
            .push(direct_struct_schemas(nodes, self.aliases));
        self.return_scopes
            .push(direct_function_returns(nodes, self.aliases));
        for node in nodes {
            self.visit_stmt(node, env);
        }
        self.return_scopes.pop();
        self.schema_scopes.pop();
    }

    fn visit_expr(&mut self, node: &Node, env: &mut BTreeMap<String, TypeAnn>) {
        match node {
            Node::StructLit { name, fields, .. } => {
                self.check_struct_literal(name, fields, env, node.span());
                for field in fields {
                    self.visit_expr(&field.value, env);
                }
            }
            Node::Let { .. }
            | Node::Const { .. }
            | Node::Assign { .. }
            | Node::LetTuple { .. }
            | Node::Return { .. }
            | Node::FnDef(..)
            | Node::Block { .. }
            | Node::If { .. }
            | Node::For { .. }
            | Node::ForEach { .. }
            | Node::Match { .. } => self.visit_stmt(node, env),
            #[cfg(feature = "std-surface")]
            Node::While { .. } => self.visit_stmt(node, env),
            _ => crate::type_checker::nerve_walk::for_each_child(node, &mut |child| {
                self.visit_expr(child, env)
            }),
        }
    }

    fn check_struct_literal(
        &mut self,
        name: &str,
        fields: &[crate::ast::StructLitField],
        env: &BTreeMap<String, TypeAnn>,
        span: crate::ast::Span,
    ) {
        let Some(schema) = self.lookup_schema(name) else {
            return;
        };
        for field in fields {
            let Some(expected) = schema.get(&field.name) else {
                continue;
            };
            let expected = self.aliases.resolve(expected);
            let TypeAnn::Array { element, length } = expected else {
                continue;
            };
            let element = self.aliases.resolve(&element);
            if !is_exact_i64(&element) {
                continue;
            }
            self.check_i64_array_field(name, &field.name, &field.value, length, env, span);
        }
    }

    fn check_i64_array_field(
        &mut self,
        struct_name: &str,
        field_name: &str,
        value: &Node,
        length: u32,
        env: &BTreeMap<String, TypeAnn>,
        span: crate::ast::Span,
    ) {
        let value = unwrap_parens(value);
        if let Node::ArrayLit { elements, span, .. } = value {
            if elements.len() != length as usize {
                self.push_error(
                    format!(
                        "array length mismatch for `{struct_name}.{field_name}`: expected {length} elements but literal has {}",
                        elements.len()
                    ),
                    *span,
                );
            }
            for element in elements {
                self.check_i64_element(element, env, struct_name, field_name);
            }
            return;
        }
        let actual = self.expr_type(value, env);
        let expected = TypeAnn::Array {
            element: Box::new(TypeAnn::ScalarI64),
            length,
        };
        if actual.as_ref().is_some_and(|actual| actual != &expected) {
            self.push_error(
                format!(
                    "array field type mismatch for `{struct_name}.{field_name}`: expected [i64; {length}]"
                ),
                span,
            );
        }
    }

    fn expr_type(&self, node: &Node, env: &BTreeMap<String, TypeAnn>) -> Option<TypeAnn> {
        let ty = match node {
            // The parser intentionally encodes bare booleans as i64 literals
            // for the existing ABI. Preserve their source spelling here so a
            // bool cannot be mistaken for an integer array cell.
            Node::Lit(Literal::Int(_), span) if self.source_span_is_bool(*span) => {
                TypeAnn::ScalarBool
            }
            Node::Lit(Literal::Int(_), _) => TypeAnn::ScalarI64,
            Node::Lit(Literal::Float(_), _) => TypeAnn::ScalarF64,
            Node::Lit(Literal::Ident(name), _) if matches!(name.as_str(), "true" | "false") => {
                TypeAnn::ScalarBool
            }
            Node::Lit(Literal::Ident(name), _) => env.get(name)?.clone(),
            Node::StructLit { name, .. } => TypeAnn::Named(name.clone()),
            Node::ArrayLit { elements, .. } => {
                let first = elements.first()?;
                let element = self.expr_type(first, env)?;
                if elements
                    .iter()
                    .skip(1)
                    .any(|item| self.expr_type(item, env).as_ref() != Some(&element))
                {
                    return None;
                }
                TypeAnn::Array {
                    element: Box::new(element),
                    length: elements.len() as u32,
                }
            }
            Node::Paren(inner, _) => return self.expr_type(inner, env),
            Node::Neg { operand, .. } | Node::BitNot { operand, .. } => {
                return self.expr_type(operand, env);
            }
            Node::Not { .. } | Node::Logical { .. } => TypeAnn::ScalarBool,
            Node::Binary {
                op, left, right, ..
            } => {
                if matches!(
                    op,
                    crate::ast::BinOp::Lt
                        | crate::ast::BinOp::Le
                        | crate::ast::BinOp::Gt
                        | crate::ast::BinOp::Ge
                        | crate::ast::BinOp::Eq
                        | crate::ast::BinOp::Ne
                ) {
                    TypeAnn::ScalarBool
                } else {
                    let left = self.expr_type(left, env)?;
                    let right = self.expr_type(right, env)?;
                    if left != right {
                        return None;
                    }
                    left
                }
            }
            #[cfg(feature = "std-surface")]
            Node::Bitwise { left, right, .. } => {
                let left = self.expr_type(left, env)?;
                let right = self.expr_type(right, env)?;
                if left != right {
                    return None;
                }
                left
            }
            Node::As { ty, .. } => self.aliases.resolve(ty),
            Node::Call { callee, .. } => self.lookup_return(callee)?,
            Node::FieldAccess {
                receiver, field, ..
            } => {
                let TypeAnn::Named(owner) = self.expr_type(receiver, env)? else {
                    return None;
                };
                self.lookup_schema(&owner)?.get(field)?.clone()
            }
            Node::IndexAccess { receiver, .. } => {
                let TypeAnn::Array { element, .. } = self.expr_type(receiver, env)? else {
                    return None;
                };
                *element
            }
            _ => return None,
        };
        Some(self.aliases.resolve(&ty))
    }

    fn lookup_schema(&self, name: &str) -> Option<StructSchema> {
        for scope in self.schema_scopes.iter().rev() {
            if let Some(schema) = scope.get(name) {
                // A duplicate in the nearest lexical scope is deliberately
                // unknown; never fall through to a same-named outer schema.
                return schema.clone();
            }
        }
        None
    }

    fn lookup_return(&self, name: &str) -> Option<TypeAnn> {
        for scope in self.return_scopes.iter().rev() {
            if let Some(ret) = scope.get(name) {
                // Untyped/generic/duplicate declarations shadow outer names,
                // but cannot provide an exact element-type fact here.
                return ret.clone();
            }
        }
        None
    }

    fn source_span_is_bool(&self, span: crate::ast::Span) -> bool {
        self.src
            .get(span.start()..span.end())
            .is_some_and(|text| matches!(text.trim(), "true" | "false"))
    }

    fn push_error(&mut self, message: String, span: crate::ast::Span) {
        self.errors.push(super::diag_from_span(
            self.src,
            self.file,
            message,
            span,
            super::TYPE_ERR_CODE,
        ));
    }
}

fn unwrap_parens(mut node: &Node) -> &Node {
    while let Node::Paren(inner, _) = node {
        node = inner;
    }
    node
}

fn is_exact_i64(ty: &TypeAnn) -> bool {
    matches!(ty, TypeAnn::ScalarI64) || matches!(ty, TypeAnn::Named(name) if name == "i64")
}

/// Check only functions whose resolved return annotation is a fixed array.
/// Scalar functions stay on the existing type-check path. The surrounding
/// nerve walk invokes this independently for nested function declarations.
fn check_fn_returns(
    fd: &crate::ast::FnDefData,
    aliases: &crate::eval::type_aliases::LocalTypeAliases,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let Some(ret) = fd.ret_type.as_ref() else {
        return;
    };
    let expected = aliases.resolve(ret);
    if !matches!(expected, TypeAnn::Array { .. }) {
        return;
    }
    for stmt in &fd.body {
        check_return_node(stmt, &expected, fd.name.as_str(), src, file, errors);
    }
    if let Some(pos) = fd.body.iter().rposition(|stmt| {
        !matches!(
            stmt,
            Node::Let { .. } | Node::LetTuple { .. } | Node::Assign { .. }
        )
    }) {
        check_tail_node(
            &fd.body[pos],
            &expected,
            fd.name.as_str(),
            src,
            file,
            errors,
        );
    }
}

fn check_return_node(
    node: &Node,
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    match node {
        Node::Return {
            value: Some(value), ..
        } => check_return_value(value, expected, fn_name, src, file, errors),
        // The outer cardinality walk invokes every nested declaration with its
        // own return annotation; never apply the parent's type here.
        Node::FnDef(..) => {}
        _ => super::nerve_walk::for_each_child(node, &mut |child| {
            check_return_node(child, expected, fn_name, src, file, errors)
        }),
    }
}

fn check_tail_node(
    node: &Node,
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    match node {
        Node::Return { .. } => {}
        Node::If {
            then_branch,
            else_branch,
            ..
        } => {
            check_tail_sequence(then_branch, expected, fn_name, src, file, errors);
            if let Some(else_branch) = else_branch {
                check_tail_sequence(else_branch, expected, fn_name, src, file, errors);
            }
        }
        Node::Block { stmts, .. } => {
            check_tail_sequence(stmts, expected, fn_name, src, file, errors);
        }
        Node::Match { arms, .. } => {
            for arm in arms {
                check_tail_node(&arm.body, expected, fn_name, src, file, errors);
            }
        }
        _ => check_return_value(node, expected, fn_name, src, file, errors),
    }
}

fn check_tail_sequence(
    stmts: &[Node],
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    if let Some(pos) = stmts.iter().rposition(|stmt| {
        !matches!(
            stmt,
            Node::Let { .. } | Node::LetTuple { .. } | Node::Assign { .. }
        )
    }) {
        check_tail_node(&stmts[pos], expected, fn_name, src, file, errors);
    }
}

fn check_return_value(
    value: &Node,
    expected: &TypeAnn,
    fn_name: &str,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let mut value = value;
    while let Node::Paren(inner, _) = value {
        value = inner;
    }
    match expected {
        TypeAnn::Array { element, length } => {
            if let Node::ArrayLit { elements, .. } = value {
                if elements.len() != *length as usize {
                    errors.push(super::diag_from_span(
                        src,
                        file,
                        format!(
                            "array length mismatch for return from `{fn_name}`: annotation [_; {length}] but value has {} elements",
                            elements.len()
                        ),
                        value.span(),
                        super::TYPE_ERR_CODE,
                    ));
                }
                for item in elements {
                    check_return_value(item, element, fn_name, src, file, errors);
                }
            }
        }
        integer if super::scalar_class_of_ann(integer) == Some(super::ScalarClass::Int) => {
            array_control_flow::check_integer_return_value(value, fn_name, src, file, errors);
        }
        _ => {}
    }
}

fn check_literal(
    name: &str,
    annotation: &TypeAnn,
    value: &Node,
    src: &str,
    file: Option<&str>,
    errors: &mut Vec<Pretty>,
) {
    let TypeAnn::Array { element, length } = annotation else {
        return;
    };
    let mut literal = value;
    while let Node::Paren(inner, _) = literal {
        literal = inner;
    }
    let Node::ArrayLit { elements, .. } = literal else {
        return;
    };
    if elements.len() != *length as usize {
        errors.push(super::diag_from_span(
            src,
            file,
            format!(
                "array length mismatch for `{name}`: annotation [_; {length}] but literal has {} elements",
                elements.len()
            ),
            literal.span(),
            super::TYPE_ERR_CODE,
        ));
    }
    if matches!(element.as_ref(), TypeAnn::Array { .. }) {
        for item in elements {
            check_literal(name, element, item, src, file, errors);
        }
    }
}
