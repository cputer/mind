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

// Part of the MIND project (Machine Intelligence Native Design).

//! Q16.16 reduction-order lint — diagnostic codes `E_NERVE_001`…`E_NERVE_005`.
//!
//! # What this closes
//!
//! A fixed-point inference tree buys cross-substrate bit-identity by pinning
//! *reduction order* and keeping IEEE-754 out of the numeric path. Both are
//! NEGATIVE properties: nothing in the source says "this is deterministic", so
//! the invariant used to rest entirely on reviewer vigilance plus a comment.
//! One hand-rolled `(a * b) >> 16` with the wrong rounding, one `f32`
//! accumulator, or one call into a backend `reduce_sum` with
//! implementation-defined associativity silently breaks byte-identity, and
//! nothing in the compiler notices.
//!
//! These five codes make the discipline machine-checked:
//!
//! | code | rule |
//! |---|---|
//! | `E_NERVE_001` | a function reachable from a bit-identity root lacks `#[determinism(BitIdentical)]` |
//! | `E_NERVE_002` | a reduction (fold / accumulate / select) lacks a `#[reduction_strategy(...)]` tag |
//! | `E_NERVE_003` | a Q16.16 multiply is written by hand instead of routed through `q16_mul` |
//! | `E_NERVE_004` | an IEEE-754 operation appears under `#[invariant(no_float_ops)]` |
//! | `E_NERVE_005` | a reduction with implementation-defined associativity is called |
//!
//! # Opt-in, and why it must be
//!
//! mindc is a general-purpose compiler. A module that never claims the
//! fixed-point contract must not be judged by it — `f64` code, tensor
//! reductions and `a * b` are all perfectly legal MIND. So the whole pass is
//! gated on an explicit annotation appearing somewhere in the file:
//!
//! * `#[determinism(BitIdentical)]` on at least one function arms rules 1, 2, 3
//!   and 5, and each annotated function is a *root* of the rule-1 reachability
//!   closure.
//! * `#[invariant(no_float_ops)]` additionally arms rule 4 for the whole file.
//!
//! A file with neither annotation exits this pass on its first statement, so
//! the cost to every other MIND source is one `bool` scan of the item list and
//! no diagnostic can be produced. That inertness is pinned by a test.
//!
//! The contract writes the no-float invariant at MODULE scope. The parser
//! accepts `#[invariant(no_float_ops)] module m { … }` but drops the attribute
//! list on the module block (`parse_module_block` takes `_attrs`), so this pass
//! reads the invariant from the FUNCTION attribute list instead: one annotated
//! function arms rule 4 for the whole file, which is the same claim written at
//! the only scope that currently survives parsing.
//!
//! deferred: module-scope arming is approximated by "any function in the file
//! declares it", because the parser discards a module block's attribute list —
//! upgrade path: carry `attrs` on the module AST node and arm rule 4 from the
//! enclosing module, so a file may hold both an annotated and an unannotated
//! module.
//!
//! # Errors, never warnings
//!
//! One byte of divergence fails a bit-identity gate; there is no "approximately
//! bit-identical". Every code here is `Severity::Error` and blocks
//! `mindc check`, matching the "these are compile errors, not warnings" wording
//! of the contract.

use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{BinOp, BitOp, FnDefData, Literal, Module, Node, Span as AstSpan, TypeAnn};
use crate::diagnostics::Diagnostic as Pretty;

use super::diag_from_span;
use super::nerve_walk::{any, walk};

/// Rule 1 — a function transitively reachable from a `#[determinism(BitIdentical)]`
/// root carries no such annotation of its own, so nothing states (or checks)
/// that its arithmetic is order-pinned.
pub const DETERMINISM_ANNOTATION_CODE: &str = "E_NERVE_001";
/// Rule 2 — a fold / accumulate / min / max / argmax over a sequence with no
/// declared reduction strategy. Untagged, a later backend is free to reassociate
/// it and the result changes in the last bit.
pub const REDUCTION_STRATEGY_CODE: &str = "E_NERVE_002";
/// Rule 3 — a Q16.16 multiply written as raw arithmetic (`(a * b) >> 16`, or
/// `a * b` on two Q16.16-typed operands) instead of the single sanctioned
/// `q16_mul` primitive, which owns the rounding and saturation policy.
pub const FORBIDDEN_Q16_MUL_CODE: &str = "E_NERVE_003";
/// Rule 4 — an IEEE-754 type, literal, cast or libm call inside a module that
/// declares `#[invariant(no_float_ops)]`.
pub const FLOAT_IN_NERVE_CODE: &str = "E_NERVE_004";
/// Rule 5 — a call to a reduction whose associativity is implementation-defined
/// (a backend `reduce_*`, or a tensor-level `sum`/`mean`, which mindc's own
/// FP-mode classifier reports as `Relaxed`). Rejected even when the caller
/// carries a reduction-strategy tag: the tag describes the caller's fold, not
/// the callee's.
pub const IMPL_DEFINED_REDUCTION_CODE: &str = "E_NERVE_005";

/// Every code this pass can raise, for exhaustive assertions in tests and for
/// documentation generators.
pub const ALL_CODES: [&str; 5] = [
    DETERMINISM_ANNOTATION_CODE,
    REDUCTION_STRATEGY_CODE,
    FORBIDDEN_Q16_MUL_CODE,
    FLOAT_IN_NERVE_CODE,
    IMPL_DEFINED_REDUCTION_CODE,
];

/// Attribute that marks a function as a bit-identity root / participant.
const ATTR_DETERMINISM: &str = "determinism";
/// The only accepted argument of [`ATTR_DETERMINISM`].
const ARG_BIT_IDENTICAL: &str = "BitIdentical";
/// Attribute that declares the per-function reduction schedule.
const ATTR_REDUCTION_STRATEGY: &str = "reduction_strategy";
/// Attribute that arms rule 4 for the whole file.
const ATTR_INVARIANT: &str = "invariant";
/// The only [`ATTR_INVARIANT`] argument this pass understands.
const ARG_NO_FLOAT_OPS: &str = "no_float_ops";

/// The reduction schedules that are order-pinned and therefore admissible.
/// `sequential` is a left-to-right fold in index order; `tree_associative_fixed`
/// is a fixed-shape binary tree — both produce one schedule per input length,
/// independent of substrate and of vector width.
const REDUCTION_STRATEGIES: [&str; 2] = ["sequential", "tree_associative_fixed"];

/// The single sanctioned Q16.16 multiply. Its own body IS the reference
/// implementation of the rounding policy, so rule 3 exempts it — otherwise the
/// primitive could never be written in MIND at all. The exemption is scoped to
/// that one body: every other function in the unit is still judged.
///
/// deferred: the exemption trusts the NAME, so a `q16_mul` whose body is not
/// the widen-shift-clamp form the contract specifies buys silence for the whole
/// unit — upgrade path: match the exempted body against the contract's shape
/// (`(a as i64 * b as i64) >> 16` with both saturation clamps) and raise
/// `E_NERVE_003` on the primitive itself when it deviates.
const Q16_MUL_PRIMITIVE: &str = "q16_mul";

/// Type names understood as "this value is Q16.16 fixed point". MIND has no
/// dedicated fixed-point scalar, so the fixed-point-ness lives in a `type`
/// alias / named annotation; these are the spellings the numerics contract
/// uses.
const Q16_TYPE_NAMES: [&str; 3] = ["Q16_16", "Q16", "q16_16"];

/// Callees whose result is not bit-reproducible across substrates: host-libm
/// transcendentals and the float-domain helpers built on them. Deliberately a
/// CLOSED list rather than a prefix heuristic — over-flagging a user function
/// named `log` would be a false error, and the failure mode of the list is
/// caught by the float TYPE / LITERAL / CAST arms, which have no such gap.
///
/// Matched only for a name this unit does NOT define: a fixed-point unit is
/// expected to ship its own integer `exp` / `sqrt` (the contract mandates
/// truncated lookup tables for exactly those), and rejecting one on its name
/// would ban the very implementation the contract requires. A definition in
/// this file is judged by its own body through the type / literal / cast arms.
/// Kept sorted for `binary_search`.
const FLOAT_MATH_CALLS: [&str; 15] = [
    "cos",
    "exp",
    "log",
    "log10",
    "log2",
    "log_softmax",
    "pow",
    "rsqrt",
    "sigmoid",
    "sin",
    "softmax",
    "sqrt",
    "tan",
    "tanh",
    "trunc",
];

/// Reductions whose associativity is implementation-defined. A backend is free
/// to fold these pairwise, in vector lanes, or across threads, so two substrates
/// legitimately disagree in the last bit. Kept sorted for `binary_search`.
const IMPL_DEFINED_REDUCTIONS: [&str; 8] = [
    "reduce_all",
    "reduce_any",
    "reduce_max",
    "reduce_mean",
    "reduce_min",
    "reduce_prod",
    "reduce_sum",
    "reduce_xor",
];

/// `true` iff `attrs` contains `#[name]` or `#[name(arg)]` for the given `arg`.
fn has_attr(fd: &FnDefData, name: &str, arg: &str) -> bool {
    fd.attrs
        .iter()
        .any(|a| a.name == name && a.args.iter().any(|v| v == arg))
}

/// The declared reduction strategy of `fd`, if the attribute is present at all.
/// `Some(None)` means "attribute present but its argument is not a schedule this
/// compiler can pin" — that is a rule-2 violation, not an exemption.
fn declared_strategy(fd: &FnDefData) -> Option<Option<&str>> {
    fd.attrs
        .iter()
        .find(|a| a.name == ATTR_REDUCTION_STRATEGY)
        .map(|a| {
            a.args
                .iter()
                .map(String::as_str)
                .find(|v| REDUCTION_STRATEGIES.contains(v))
        })
}

/// Strip the module qualifier from a callee path: `backend::reduce_sum` →
/// `reduce_sum`. Both `::` and `.` are accepted because MIND spells qualified
/// paths both ways depending on surface (`std.vec` import vs `Type::method`).
fn base_callee(callee: &str) -> &str {
    let after_colons = callee.rsplit("::").next().unwrap_or(callee);
    after_colons.rsplit('.').next().unwrap_or(after_colons)
}

/// `true` for the float scalar annotations. Tensors of `f32`/`f64` count too:
/// their element type is IEEE-754 whatever the container.
fn is_float_ann(ty: &TypeAnn) -> bool {
    match ty {
        TypeAnn::ScalarF32 | TypeAnn::ScalarF64 => true,
        TypeAnn::Tensor { dtype, .. } | TypeAnn::DiffTensor { dtype, .. } => {
            dtype == "f32" || dtype == "f64"
        }
        TypeAnn::Slice { element, .. } | TypeAnn::Array { element, .. } => is_float_ann(element),
        TypeAnn::Ref { target, .. } => is_float_ann(target),
        TypeAnn::Tuple { elements } => elements.iter().any(is_float_ann),
        TypeAnn::Generic { args, .. } => args.iter().any(is_float_ann),
        _ => false,
    }
}

/// `true` iff `ty` names a Q16.16 fixed-point value.
fn is_q16_ann(ty: &TypeAnn) -> bool {
    matches!(ty, TypeAnn::Named(n) if Q16_TYPE_NAMES.contains(&n.as_str()))
}

/// Peel `(expr)` and `expr as T` down to the value being named.
fn strip_wrappers(node: &Node) -> &Node {
    match node {
        Node::Paren(inner, _) => strip_wrappers(inner),
        Node::As { expr, .. } => strip_wrappers(expr),
        other => other,
    }
}

/// The identifier a node ultimately names, if it names one.
fn ident_of(node: &Node) -> Option<&str> {
    match strip_wrappers(node) {
        Node::Lit(Literal::Ident(n), _) => Some(n.as_str()),
        _ => None,
    }
}

/// `true` iff `node` is (or contains) a scalar multiply.
fn contains_mul(node: &Node) -> bool {
    any(node, &|n| matches!(n, Node::Binary { op: BinOp::Mul, .. }))
}

/// `true` iff `node` is the integer literal 16 — the Q16.16 fractional width.
fn is_lit_16(node: &Node) -> bool {
    matches!(strip_wrappers(node), Node::Lit(Literal::Int(16), _))
}

/// Every top-level and nested function definition in `module`, in source order.
fn collect_fns(module: &Module) -> Vec<(&FnDefData, AstSpan)> {
    let mut out = Vec::new();
    for item in &module.items {
        walk(item, &mut |n| {
            if let Node::FnDef(fd, span) = n {
                out.push((fd.as_ref(), *span));
            }
        });
    }
    out
}

/// Run the Q16.16 numerics lint over `module`, appending diagnostics to `errs`.
///
/// Returns immediately (no allocation beyond the item scan) for any module that
/// carries neither opt-in annotation — see the module docs on inertness.
pub(super) fn check_nerve_numerics(
    module: &Module,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    let fns = collect_fns(module);
    let armed = fns.iter().any(|(fd, _)| {
        has_attr(fd, ATTR_DETERMINISM, ARG_BIT_IDENTICAL)
            || has_attr(fd, ATTR_INVARIANT, ARG_NO_FLOAT_OPS)
    });
    if !armed {
        return;
    }
    let no_float = fns
        .iter()
        .any(|(fd, _)| has_attr(fd, ATTR_INVARIANT, ARG_NO_FLOAT_OPS));

    // Names this unit DEFINES itself. Rule 5 keys on it so the rule rejects a
    // reduction the BACKEND provides, never one written here — see
    // `check_impl_defined_reductions` on why matching a bare name would be
    // unsound in both directions.
    let defined_here: BTreeSet<&str> = fns.iter().map(|(fd, _)| fd.name.as_str()).collect();

    check_determinism_closure(&fns, src, file, errs);
    for (fd, span) in &fns {
        check_reduction_tag(fd, *span, src, file, errs);
        check_q16_multiply(fd, src, file, errs);
        check_impl_defined_reductions(fd, &defined_here, src, file, errs);
        if no_float {
            check_no_float_ops(fd, *span, &defined_here, src, file, errs);
        }
    }
}

// ---------------------------------------------------------------------------
// Rule 1 — `E_NERVE_001`
// ---------------------------------------------------------------------------

/// Every function transitively reachable from a `#[determinism(BitIdentical)]`
/// root must carry the annotation itself.
///
/// The closure is computed over calls to functions DEFINED IN THIS FILE. A call
/// that leaves the file is out of this pass's knowledge — reporting it would be
/// a guess, and a guess that fires is worse than a gap that is documented.
///
/// deferred: the reachability closure stops at a module boundary, so an
/// imported callee is neither attested nor flagged — upgrade path: run the pass
/// over the resolved cross-module item table (`cross-module-imports`) and seed
/// the queue with every root in the whole compilation unit rather than one
/// file.
fn check_determinism_closure(
    fns: &[(&FnDefData, AstSpan)],
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    let mut defined: BTreeMap<&str, (&FnDefData, AstSpan)> = BTreeMap::new();
    for (fd, span) in fns {
        defined.entry(fd.name.as_str()).or_insert((*fd, *span));
    }

    let mut queue: Vec<&str> = fns
        .iter()
        .filter(|(fd, _)| has_attr(fd, ATTR_DETERMINISM, ARG_BIT_IDENTICAL))
        .map(|(fd, _)| fd.name.as_str())
        .collect();
    let mut seen: BTreeSet<&str> = queue.iter().copied().collect();
    let mut violations: BTreeMap<&str, AstSpan> = BTreeMap::new();

    while let Some(name) = queue.pop() {
        let Some((fd, _)) = defined.get(name) else {
            continue;
        };
        let mut callees: Vec<&str> = Vec::new();
        for stmt in &fd.body {
            walk(stmt, &mut |n| {
                if let Node::Call { callee, .. } = n {
                    callees.push(base_callee(callee));
                }
            });
        }
        for callee in callees {
            let Some((cfd, cspan)) = defined.get(callee) else {
                continue;
            };
            if !seen.insert(callee) {
                continue;
            }
            if has_attr(cfd, ATTR_DETERMINISM, ARG_BIT_IDENTICAL) {
                queue.push(callee);
            } else {
                // The leaf violation: report here and do NOT descend past it.
                // Its own callees are unattested for the same reason, and one
                // diagnostic per missing annotation is the actionable count.
                violations.insert(callee, *cspan);
            }
        }
    }

    for (name, span) in violations {
        errs.push(diag_from_span(
            src,
            file,
            format!(
                "function `{name}` is reachable from a `#[determinism(BitIdentical)]` root but \
                 carries no determinism annotation; its arithmetic order is unattested — add \
                 `#[determinism(BitIdentical)]` above this signature"
            ),
            span,
            DETERMINISM_ANNOTATION_CODE,
        ));
    }
}

// ---------------------------------------------------------------------------
// Rule 2 — `E_NERVE_002`
// ---------------------------------------------------------------------------

/// `true` iff `stmts` (a loop body) folds a value across iterations.
///
/// Two shapes count, and only these two:
///
/// * ACCUMULATE — `acc = acc <op> …`: the assignment reads its own target, so
///   the result depends on the visit order of the loop.
/// * SELECT — `if … best … { best = … }`: the running min / max / argmax, whose
///   result depends on tie-break order.
///
/// A loop that assigns without reading its target is a scatter/store, not a
/// reduction, and is deliberately NOT flagged: making every loop need an
/// attribute would turn the rule into noise, and noise gets suppressed.
///
/// The ACCUMULATE shape deliberately OVER-approximates in one place: an
/// explicit `while` induction step (`i = i + 1`) reads its own target and is
/// therefore reported, even though a counter is order-pinned by construction.
/// That is the fail-closed direction — the answer is one truthful
/// `#[reduction_strategy(sequential)]`, whereas a rule that carved out
/// self-increment would also have to decide whether `acc = acc + x[i]` under
/// `while acc < limit` is a counter or a fold, and getting THAT wrong is a
/// silent miss on a bit-identity gate.
fn body_is_reduction(stmts: &[Node]) -> bool {
    let mut found = false;
    for stmt in stmts {
        walk(stmt, &mut |n| {
            if found {
                return;
            }
            match n {
                Node::Assign { name, value, .. } => {
                    if any(
                        value,
                        &|inner| matches!(inner, Node::Lit(Literal::Ident(i), _) if i == name),
                    ) {
                        found = true;
                    }
                }
                Node::If {
                    cond,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    let mut targets: Vec<&str> = Vec::new();
                    let mut branches: Vec<&[Node]> = vec![then_branch.as_slice()];
                    if let Some(e) = else_branch {
                        branches.push(e.as_slice());
                    }
                    for branch in branches {
                        for s in branch {
                            if let Node::Assign { name, .. } = s {
                                targets.push(name.as_str());
                            }
                        }
                    }
                    for t in targets {
                        if any(
                            cond,
                            &|inner| matches!(inner, Node::Lit(Literal::Ident(i), _) if i == t),
                        ) {
                            found = true;
                        }
                    }
                }
                _ => {}
            }
        });
    }
    found
}

/// A reduction anywhere in `fd` obliges `fd` to declare its schedule.
fn check_reduction_tag(
    fd: &FnDefData,
    span: AstSpan,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    let mut reduces = false;
    for stmt in &fd.body {
        walk(stmt, &mut |n| {
            if reduces {
                return;
            }
            let body = match n {
                Node::For { body, .. } | Node::ForEach { body, .. } => body,
                #[cfg(feature = "std-surface")]
                Node::While { body, .. } => body,
                _ => return,
            };
            if body_is_reduction(body) {
                reduces = true;
            }
        });
    }
    if !reduces {
        return;
    }
    let msg = match declared_strategy(fd) {
        Some(Some(_)) => return,
        Some(None) => format!(
            "function `{}` declares a `#[{ATTR_REDUCTION_STRATEGY}(...)]` this compiler cannot \
             pin; the only order-pinned schedules are `sequential` and `tree_associative_fixed`",
            fd.name
        ),
        None => format!(
            "function `{}` reduces over a sequence but declares no reduction strategy; a \
             reduction with no pinned schedule may be reassociated by a backend and stops being \
             bit-identical — declare `#[{ATTR_REDUCTION_STRATEGY}(sequential)]` or \
             `#[{ATTR_REDUCTION_STRATEGY}(tree_associative_fixed)]`",
            fd.name
        ),
    };
    errs.push(diag_from_span(
        src,
        file,
        msg,
        span,
        REDUCTION_STRATEGY_CODE,
    ));
}

// ---------------------------------------------------------------------------
// Rule 3 — `E_NERVE_003`
// ---------------------------------------------------------------------------

/// The only legal Q16.16 multiply is `q16_mul`.
///
/// Two hand-rolled forms are caught:
///
/// * `(… * …) >> 16` — the scale-correcting shift, whatever the operand types
///   were cast to on the way (`(a as i64 * b as i64) >> 16` included). The
///   shift-by-16 is what makes this a *fixed-point* multiply rather than an
///   integer one, so it is the reliable marker.
/// * `a * b` where either operand is declared Q16.16 — a multiply with the
///   scale correction MISSING, which the type system cannot see because the
///   fixed-point-ness lives in a type alias.
fn check_q16_multiply(fd: &FnDefData, src: &str, file: Option<&str>, errs: &mut Vec<Pretty>) {
    if fd.name == Q16_MUL_PRIMITIVE {
        return;
    }
    let mut q16_names: BTreeSet<&str> = fd
        .params
        .iter()
        .filter(|p| is_q16_ann(&p.ty))
        .map(|p| p.name.as_str())
        .collect();
    for stmt in &fd.body {
        walk(stmt, &mut |n| {
            if let Node::Let {
                name,
                ann: Some(ty),
                ..
            } = n
            {
                if is_q16_ann(ty) {
                    q16_names.insert(name.as_str());
                }
            }
        });
    }

    let mut hits: Vec<AstSpan> = Vec::new();
    for stmt in &fd.body {
        walk(stmt, &mut |n| match n {
            Node::Bitwise {
                op: BitOp::Shr,
                left,
                right,
                span,
            } if is_lit_16(right) && contains_mul(left) => hits.push(*span),
            Node::Binary {
                op: BinOp::Mul,
                left,
                right,
                span,
            } => {
                let tainted = [left, right]
                    .iter()
                    .any(|s| ident_of(s).is_some_and(|i| q16_names.contains(i)));
                if tainted {
                    hits.push(*span);
                }
            }
            _ => {}
        });
    }
    for span in hits {
        errs.push(diag_from_span(
            src,
            file,
            format!(
                "forbidden Q16.16 multiply pattern in `{}`: the rounding and saturation policy \
                 lives in `{Q16_MUL_PRIMITIVE}`, and a hand-written multiply silently adopts a \
                 different one — use `{Q16_MUL_PRIMITIVE}(a, b)`",
                fd.name
            ),
            span,
            FORBIDDEN_Q16_MUL_CODE,
        ));
    }
}

// ---------------------------------------------------------------------------
// Rule 4 — `E_NERVE_004`
// ---------------------------------------------------------------------------

/// No IEEE-754 anywhere under `#[invariant(no_float_ops)]`.
///
/// Scores convert to decimal at the wire boundary; the inference internals stay
/// in Q16.16. Four surfaces are checked — signature types, `let` annotations,
/// `as` casts, and float literals — plus calls into the host libm, so a float
/// cannot enter by type, by value, by conversion, or by function result.
fn check_no_float_ops(
    fd: &FnDefData,
    span: AstSpan,
    defined_here: &BTreeSet<&str>,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    let sig_float = fd.params.iter().any(|p| is_float_ann(&p.ty))
        || fd.ret_type.as_ref().is_some_and(is_float_ann);
    if sig_float {
        errs.push(diag_from_span(
            src,
            file,
            format!(
                "function `{}` has an IEEE-754 type in its signature but the module declares \
                 `#[{ATTR_INVARIANT}({ARG_NO_FLOAT_OPS})]`; inference internals stay in Q16.16 and \
                 convert to decimal only at the wire boundary",
                fd.name
            ),
            span,
            FLOAT_IN_NERVE_CODE,
        ));
    }

    let mut hits: Vec<(AstSpan, String)> = Vec::new();
    for stmt in &fd.body {
        walk(stmt, &mut |n| match n {
            Node::Lit(Literal::Float(_), s) => {
                hits.push((*s, "a floating-point literal".to_string()));
            }
            Node::As { ty, span: s, .. } if is_float_ann(ty) => {
                hits.push((*s, "a cast to an IEEE-754 type".to_string()));
            }
            Node::Let {
                ann: Some(ty),
                span: s,
                ..
            } if is_float_ann(ty) => {
                hits.push((*s, "an IEEE-754 binding".to_string()));
            }
            Node::Call {
                callee, span: s, ..
            } if FLOAT_MATH_CALLS.binary_search(&base_callee(callee)).is_ok()
                && !defined_here.contains(base_callee(callee)) =>
            {
                hits.push((*s, format!("a call to the host-libm helper `{callee}`")));
            }
            _ => {}
        });
    }
    for (s, what) in hits {
        errs.push(diag_from_span(
            src,
            file,
            format!(
                "{what} inside `{}` violates `#[{ATTR_INVARIANT}({ARG_NO_FLOAT_OPS})]`: IEEE-754 \
                 results are not byte-identical across substrates",
                fd.name
            ),
            s,
            FLOAT_IN_NERVE_CODE,
        ));
    }
}

// ---------------------------------------------------------------------------
// Rule 5 — `E_NERVE_005`
// ---------------------------------------------------------------------------

/// Reject reductions whose associativity the backend chooses.
///
/// NAME-TRUST. The rule cannot key on the callee name alone. `reduce_sum` is a
/// perfectly ordinary identifier: a unit is free to DEFINE one, and a definition
/// carried in this file is already governed by rules 1-3 (it must be annotated,
/// it must declare its schedule, its multiplies must route through `q16_mul`).
/// Rejecting it on the name would both ban a legal MIND program and report a
/// pinned in-source fold as implementation-defined. So the rule fires only for a
/// name on the list that this unit does NOT define — i.e. one the backend
/// supplies, which is exactly the case the contract is about. The list itself is
/// CLOSED for the same reason: a `reduce_*` prefix heuristic would flag a user
/// helper whose body it never read.
///
/// The tensor-level `sum` / `mean` forms are included on mindc's own authority:
/// the FP-mode classifier reports an f32 tensor `Sum`/`Mean` as `Relaxed`
/// because it lowers to a reassociating `vector.reduction`. A reduction that
/// the compiler already knows is `Relaxed` cannot be the basis of a
/// bit-identity claim, whatever strategy the caller declares.
fn check_impl_defined_reductions(
    fd: &FnDefData,
    defined_here: &BTreeSet<&str>,
    src: &str,
    file: Option<&str>,
    errs: &mut Vec<Pretty>,
) {
    let mut hits: Vec<(AstSpan, String)> = Vec::new();
    for stmt in &fd.body {
        walk(stmt, &mut |n| match n {
            Node::Call { callee, span, .. }
                if IMPL_DEFINED_REDUCTIONS
                    .binary_search(&base_callee(callee))
                    .is_ok()
                    && !defined_here.contains(base_callee(callee)) =>
            {
                hits.push((*span, callee.clone()));
            }
            Node::CallTensorSum { span, .. } => hits.push((*span, "tensor sum".to_string())),
            Node::CallTensorMean { span, .. } => hits.push((*span, "tensor mean".to_string())),
            _ => {}
        });
    }
    for (span, name) in hits {
        errs.push(diag_from_span(
            src,
            file,
            format!(
                "`{name}` has implementation-defined associativity and is not bit-identity safe, \
                 so it is rejected inside `{}` even with a reduction-strategy tag; use a pinned \
                 in-source fold (`q16_sum`) or a `tree_associative_fixed` reduction",
                fd.name
            ),
            span,
            IMPL_DEFINED_REDUCTION_CODE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_tables_are_sorted_for_binary_search() {
        assert!(
            FLOAT_MATH_CALLS.windows(2).all(|w| w[0] < w[1]),
            "FLOAT_MATH_CALLS must be sorted ascending and unique"
        );
        assert!(
            IMPL_DEFINED_REDUCTIONS.windows(2).all(|w| w[0] < w[1]),
            "IMPL_DEFINED_REDUCTIONS must be sorted ascending and unique"
        );
    }

    #[test]
    fn base_callee_strips_both_qualifier_spellings() {
        assert_eq!(base_callee("backend::reduce_sum"), "reduce_sum");
        assert_eq!(base_callee("kernels.matmul_q16.dot_k"), "dot_k");
        assert_eq!(base_callee("q16_mul"), "q16_mul");
    }
}
