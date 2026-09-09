// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Opt-in expression facts produced by the existing type checker.
//!
//! The ordinary checker does not install this sink.  The canonical source
//! bridge installs it around its one type-checking pass, so legacy checking
//! and lowering do not allocate or walk a second type model.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::ast::{BinOp, Literal, Node, Param, Span, TypeAnn};
use crate::types::ValueType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckedFact {
    /// The checker established this expression's value type through an exact
    /// supported rule.  The lowerer may still apply an explicitly supported
    /// ABI widening (for example i32 literal to an i64 parameter).
    Exact(ValueType),
    /// The checker accepted the source through a fallback or lossy rule.  A
    /// canonical call/return must refuse this rather than reinterpret the
    /// fallback as producer metadata.
    Unknown,
}

type SpanKey = (usize, usize);

fn span_key(span: Span) -> SpanKey {
    (span.start(), span.end())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FactKey {
    pub(crate) function: Option<SpanKey>,
    pub(crate) expression: SpanKey,
}

impl FactKey {
    pub(crate) fn new(function: Option<Span>, expression: Span) -> Self {
        Self {
            function: function.map(span_key),
            expression: span_key(expression),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckedFactTable {
    facts: BTreeMap<FactKey, CheckedFact>,
}

impl CheckedFactTable {
    pub(crate) fn get(&self, function: Option<Span>, expression: Span) -> Option<&CheckedFact> {
        self.facts.get(&FactKey::new(function, expression))
    }
}

struct Sink {
    table: CheckedFactTable,
    scope: Option<SpanKey>,
    bindings: BTreeMap<Option<SpanKey>, BTreeMap<String, CheckedFact>>,
}

impl Sink {
    fn new() -> Self {
        Self {
            table: CheckedFactTable::default(),
            scope: None,
            bindings: BTreeMap::new(),
        }
    }

    fn key(&self, expression: Span) -> FactKey {
        FactKey {
            function: self.scope,
            expression: span_key(expression),
        }
    }

    fn record(&mut self, expression: Span, fact: CheckedFact) {
        let key = self.key(expression);
        match self.table.facts.get_mut(&key) {
            None => {
                self.table.facts.insert(key, fact);
            }
            Some(existing) if *existing == fact => {}
            Some(existing) => *existing = CheckedFact::Unknown,
        }
    }

    fn set_binding(&mut self, name: String, fact: CheckedFact) {
        self.bindings
            .entry(self.scope)
            .or_default()
            .insert(name, fact);
    }

    fn binding(&self, name: &str) -> Option<CheckedFact> {
        self.bindings
            .get(&self.scope)
            .and_then(|bindings| bindings.get(name))
            .cloned()
            .or_else(|| {
                self.bindings
                    .get(&None)
                    .and_then(|bindings| bindings.get(name))
                    .cloned()
            })
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

pub(crate) fn active() -> bool {
    ACTIVE.with(|cell| cell.borrow().is_some())
}

pub(crate) fn record(node: &Node, result: &Result<(ValueType, Span), super::TypeErrSpan>) {
    if active() {
        record_checked_result(node, result);
    }
}

/// Installs the canonical-only sink.  The caller must call [`Guard::finish`]
/// after checking to take the collected table.
pub(crate) struct Guard {
    previous: Option<Sink>,
    finished: bool,
}

impl Guard {
    pub(crate) fn install() -> Self {
        let previous = ACTIVE.with(|cell| cell.borrow_mut().replace(Sink::new()));
        Self {
            previous,
            finished: false,
        }
    }

    pub(crate) fn finish(mut self) -> CheckedFactTable {
        let sink = ACTIVE
            .with(|cell| cell.borrow_mut().take())
            .expect("canonical fact sink was not installed");
        // A nested canonical check temporarily shadows the parent sink.  A
        // successful finish must restore that parent just like Drop does;
        // otherwise the inner finish leaves the outer checker with no sink.
        ACTIVE.with(|cell| {
            *cell.borrow_mut() = self.previous.take();
        });
        self.finished = true;
        sink.table
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.finished {
            ACTIVE.with(|cell| {
                *cell.borrow_mut() = self.previous.take();
            });
        }
    }
}

/// Installs the lexical function scope for facts collected while checking a
/// function body.  Nested scopes restore the parent on drop.
pub(crate) struct FunctionGuard {
    previous: Option<SpanKey>,
}

impl FunctionGuard {
    pub(crate) fn install(span: Span) -> Self {
        let previous = ACTIVE.with(|cell| {
            let mut sink = cell.borrow_mut();
            sink.as_mut()
                .and_then(|sink| sink.scope.replace(span_key(span)))
        });
        Self { previous }
    }
}

pub(crate) fn enter_function(span: Span, params: &[Param]) -> FunctionGuard {
    let guard = FunctionGuard::install(span);
    for param in params {
        set_binding_type(&param.name, super::valuetype_from_ann(&param.ty));
    }
    guard
}

/// Whether a successful call result came from a scalar declaration that the
/// canonical bridge can bind. The loose `ScalarI64` fallback for an unknown or
/// aggregate declaration is intentionally excluded.
pub(super) fn call_has_exact_scalar_result(callee: &str, span: Span) -> bool {
    let ret_type = crate::project::active_module_table::qualified_import_fn(span, callee)
        .and_then(|sig| sig.ret_type)
        .or_else(|| super::cm_lookup_fn(callee).and_then(|sig| sig.ret_type))
        .or_else(|| super::intra_lookup_fn(callee).and_then(|sig| sig.ret_type));
    ret_type
        .as_ref()
        .and_then(super::valuetype_from_ann)
        .is_some_and(|ty| ty.is_scalar())
}

impl Drop for FunctionGuard {
    fn drop(&mut self) {
        ACTIVE.with(|cell| {
            if let Some(sink) = cell.borrow_mut().as_mut() {
                sink.scope = self.previous.take();
            }
        });
    }
}

pub(crate) fn record_exact(span: Span, ty: ValueType) {
    ACTIVE.with(|cell| {
        if let Some(sink) = cell.borrow_mut().as_mut() {
            sink.record(span, CheckedFact::Exact(ty));
        }
    });
}

pub(crate) fn record_unknown(span: Span) {
    ACTIVE.with(|cell| {
        if let Some(sink) = cell.borrow_mut().as_mut() {
            sink.record(span, CheckedFact::Unknown);
        }
    });
}

pub(crate) fn set_binding(name: &str, fact: CheckedFact) {
    ACTIVE.with(|cell| {
        if let Some(sink) = cell.borrow_mut().as_mut() {
            sink.set_binding(name.to_string(), fact);
        }
    });
}

pub(crate) fn set_binding_type(name: &str, ty: Option<ValueType>) {
    set_binding(
        name,
        ty.map(CheckedFact::Exact).unwrap_or(CheckedFact::Unknown),
    );
}

pub(crate) fn bind(name: &str, span: Span) {
    set_binding(name, exact_or_unknown(span));
}

pub(crate) fn binding(name: &str) -> Option<CheckedFact> {
    ACTIVE.with(|cell| cell.borrow().as_ref().and_then(|sink| sink.binding(name)))
}

pub(crate) fn current_fact(span: Span) -> Option<CheckedFact> {
    ACTIVE.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|sink| sink.table.facts.get(&sink.key(span)).cloned())
    })
}

pub(crate) fn exact_or_unknown(span: Span) -> CheckedFact {
    current_fact(span).unwrap_or(CheckedFact::Unknown)
}

/// Record only facts whose *existing checker result* is exact for the
/// canonical scalar slice.  This deliberately classifies the result; it does
/// not infer a type from the AST.  The checker remains the authority for the
/// value in `result`, while this allowlist rejects its benign scalar
/// placeholders and lossy aggregate fallbacks.
pub(crate) fn record_checked_result(
    node: &Node,
    result: &Result<(ValueType, Span), super::TypeErrSpan>,
) {
    if !active() {
        return;
    }
    let Ok((ty, _)) = result else {
        return;
    };
    let exact = match node {
        Node::Lit(Literal::Int(_), _) | Node::Lit(Literal::Float(_), _) => true,
        Node::Lit(Literal::Ident(name), _) => {
            matches!(binding(name), Some(CheckedFact::Exact(_)))
        }
        Node::Paren(inner, _) | Node::Neg { operand: inner, .. } => {
            matches!(current_fact(inner.span()), Some(CheckedFact::Exact(_)))
        }
        Node::As {
            expr, ty: target, ..
        } => {
            matches!(
                target,
                TypeAnn::ScalarI32
                    | TypeAnn::ScalarI64
                    | TypeAnn::ScalarF32
                    | TypeAnn::ScalarF64
                    | TypeAnn::ScalarBool
                    | TypeAnn::ScalarU32
            ) && matches!(current_fact(expr.span()), Some(CheckedFact::Exact(_)))
        }
        Node::Binary {
            op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod,
            left,
            right,
            ..
        } => {
            matches!(current_fact(left.span()), Some(CheckedFact::Exact(_)))
                && matches!(current_fact(right.span()), Some(CheckedFact::Exact(_)))
        }
        Node::Call { callee, span, .. } => {
            call_has_exact_scalar_result(callee, *span) && ty.is_scalar()
        }
        _ => false,
    };
    if exact {
        record_exact(node.span(), ty.clone());
    } else {
        record_unknown(node.span());
    }
}

#[cfg(test)]
mod tests {
    use super::{CheckedFact, Guard, active, current_fact, record_exact};
    use crate::ast::Span;
    use crate::types::ValueType;

    #[test]
    fn finished_nested_sink_restores_parent_for_following_facts() {
        let outer_span = Span::new(1, 2);
        let after_inner_span = Span::new(3, 4);
        let inner_span = Span::new(5, 6);

        let outer = Guard::install();
        record_exact(outer_span, ValueType::ScalarI64);
        let inner = Guard::install();
        record_exact(inner_span, ValueType::ScalarF64);
        let inner_table = inner.finish();

        assert_eq!(
            inner_table.get(None, inner_span),
            Some(&CheckedFact::Exact(ValueType::ScalarF64))
        );
        assert_eq!(
            current_fact(outer_span),
            Some(CheckedFact::Exact(ValueType::ScalarI64))
        );
        record_exact(after_inner_span, ValueType::ScalarI32);
        let outer_table = outer.finish();
        assert_eq!(
            outer_table.get(None, outer_span),
            Some(&CheckedFact::Exact(ValueType::ScalarI64))
        );
        assert_eq!(
            outer_table.get(None, after_inner_span),
            Some(&CheckedFact::Exact(ValueType::ScalarI32))
        );
        assert!(!active());
    }
}
