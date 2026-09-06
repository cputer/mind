// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Typed string equality normalization before recursive expression lowering.
//!
//! Only locally constructed immutable string values are proof. Parameter and
//! return annotations share the loose i64 ABI with opaque handles, so this pass
//! never treats an annotation alone as permission to dereference a String
//! record. Any assignment to a name makes that name ineligible throughout its
//! declaration body; this conservative rule avoids branch, loop, and shadowing
//! ambiguity while retaining literal and immutable-alias equality.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::ast::{BinOp, Literal, Module, Node, Pattern};

type StringEnv = BTreeSet<String>;
type Targets = BTreeSet<usize>;
type Assigned = BTreeSet<String>;
type Ordinals = BTreeMap<usize, usize>;

fn string_result_method(name: &str) -> bool {
    matches!(
        name,
        "slice"
            | "substring"
            | "trim"
            | "trim_start"
            | "trim_end"
            | "to_lowercase"
            | "to_uppercase"
            | "to_lower"
            | "to_upper"
            | "replace"
            | "concat"
            | "repeat"
    )
}

/// This deliberately accepts only values whose string-record provenance is
/// visible locally. A named return declaration or opaque i64 handle alone is
/// not proof that the value has the `{addr,len,cap}` String layout.
fn is_string_value(mut node: &Node, env: &StringEnv) -> bool {
    loop {
        match node {
            Node::Lit(Literal::Str(_), _) => return true,
            Node::Lit(Literal::Ident(name), _) => return env.contains(name),
            Node::Paren(inner, _) => node = inner,
            Node::MethodCall {
                receiver, method, ..
            } if string_result_method(method) => node = receiver,
            _ => return false,
        }
    }
}

fn remove_pattern_bindings(pattern: &Pattern, env: &mut StringEnv) {
    let mut pending = vec![pattern];
    while let Some(pattern) = pending.pop() {
        match pattern {
            Pattern::Ident(name) => {
                env.remove(name);
            }
            Pattern::EnumVariant { args, .. } | Pattern::Tuple(args) => pending.extend(args),
            Pattern::EnumStruct { fields, .. } => {
                pending.extend(fields.iter().map(|(_, pattern)| pattern));
            }
            Pattern::Literal(_) | Pattern::Wildcard => {}
        }
    }
}

fn children_ref(node: &Node) -> Vec<&Node> {
    match node {
        Node::FnDef(fd, _) => fd.body.iter().collect(),
        Node::Closure(data, _) => data.body.iter().collect(),
        _ => super::closures::node_children_ref(node),
    }
}

fn children_mut(node: &mut Node) -> Vec<&mut Node> {
    match node {
        Node::FnDef(fd, _) => fd.body.iter_mut().collect(),
        Node::Closure(data, _) => data.body.iter_mut().collect(),
        _ => super::closures::node_children_mut(node),
    }
}

fn assigned_in(stmts: &[Node]) -> Assigned {
    let mut assigned = Assigned::new();
    let mut pending: Vec<&Node> = stmts.iter().rev().collect();
    while let Some(node) = pending.pop() {
        match node {
            Node::Assign { name, value, .. } => {
                assigned.insert(name.clone());
                pending.push(value);
            }
            // Each declaration owns its own assignment analysis.
            Node::FnDef(..) | Node::Closure(..) => {}
            _ => {
                let children = super::closures::node_children_ref(node);
                pending.extend(children.into_iter().rev());
            }
        }
    }
    assigned
}

/// Give every original equality node a deterministic preorder number. Pointer
/// identity is used only inside this immutable AST snapshot; the numbers, not
/// pointers or source-local spans, are carried into the rewritten clone.
fn equality_ordinals(module: &Module) -> Ordinals {
    let mut ordinals = Ordinals::new();
    let mut next = 0;
    let mut pending: Vec<&Node> = module.items.iter().rev().collect();
    while let Some(node) = pending.pop() {
        if matches!(
            node,
            Node::Binary {
                op: BinOp::Eq | BinOp::Ne,
                ..
            }
        ) {
            ordinals.insert(node as *const Node as usize, next);
            next += 1;
        }
        let children = children_ref(node);
        pending.extend(children.into_iter().rev());
    }
    ordinals
}

enum Task<'a> {
    Stmts {
        stmts: &'a [Node],
        index: usize,
        env: StringEnv,
        assigned: Rc<Assigned>,
    },
    Node {
        node: &'a Node,
        env: StringEnv,
        assigned: Rc<Assigned>,
    },
}

fn push_stmts<'a>(pending: &mut Vec<Task<'a>>, stmts: &'a [Node], env: StringEnv) {
    pending.push(Task::Stmts {
        stmts,
        index: 0,
        env,
        assigned: Rc::new(assigned_in(stmts)),
    });
}

/// Record equality nodes with two locally proven string operands. This visitor
/// owns an explicit heap worklist so deeply nested std modules do not consume
/// the compiler thread stack. Since assigned names are excluded for their whole
/// declaration, branch-local facts never need an optimistic control-flow join.
fn collect_targets(module: &Module, ordinals: &Ordinals) -> Targets {
    let mut targets = Targets::new();
    let mut pending = Vec::new();
    push_stmts(&mut pending, &module.items, StringEnv::new());
    while let Some(task) = pending.pop() {
        match task {
            Task::Stmts {
                stmts,
                index,
                mut env,
                assigned,
            } => {
                let Some(stmt) = stmts.get(index) else {
                    continue;
                };
                match stmt {
                    Node::Let { name, value, .. } => {
                        let value_env = env.clone();
                        if !assigned.contains(name) && is_string_value(value, &env) {
                            env.insert(name.clone());
                        } else {
                            env.remove(name);
                        }
                        pending.push(Task::Stmts {
                            stmts,
                            index: index + 1,
                            env,
                            assigned: assigned.clone(),
                        });
                        pending.push(Task::Node {
                            node: value,
                            env: value_env,
                            assigned,
                        });
                    }
                    Node::Assign { name, value, .. } => {
                        let value_env = env.clone();
                        env.remove(name);
                        pending.push(Task::Stmts {
                            stmts,
                            index: index + 1,
                            env,
                            assigned: assigned.clone(),
                        });
                        pending.push(Task::Node {
                            node: value,
                            env: value_env,
                            assigned,
                        });
                    }
                    Node::FnDef(fd, _) => {
                        pending.push(Task::Stmts {
                            stmts,
                            index: index + 1,
                            env,
                            assigned,
                        });
                        push_stmts(&mut pending, &fd.body, StringEnv::new());
                    }
                    _ => {
                        pending.push(Task::Stmts {
                            stmts,
                            index: index + 1,
                            env: env.clone(),
                            assigned: assigned.clone(),
                        });
                        pending.push(Task::Node {
                            node: stmt,
                            env,
                            assigned,
                        });
                    }
                }
            }
            Task::Node {
                node,
                env,
                assigned,
            } => match node {
                Node::Block { stmts, .. } | Node::Region { body: stmts, .. } => {
                    pending.push(Task::Stmts {
                        stmts,
                        index: 0,
                        env,
                        assigned,
                    });
                }
                Node::If {
                    cond,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    if let Some(branch) = else_branch {
                        pending.push(Task::Stmts {
                            stmts: branch,
                            index: 0,
                            env: env.clone(),
                            assigned: assigned.clone(),
                        });
                    }
                    pending.push(Task::Stmts {
                        stmts: then_branch,
                        index: 0,
                        env: env.clone(),
                        assigned: assigned.clone(),
                    });
                    pending.push(Task::Node {
                        node: cond,
                        env,
                        assigned,
                    });
                }
                Node::For {
                    var,
                    start,
                    end,
                    body,
                    ..
                } => {
                    let mut body_env = env.clone();
                    body_env.remove(var);
                    pending.push(Task::Stmts {
                        stmts: body,
                        index: 0,
                        env: body_env,
                        assigned: assigned.clone(),
                    });
                    pending.push(Task::Node {
                        node: end,
                        env: env.clone(),
                        assigned: assigned.clone(),
                    });
                    pending.push(Task::Node {
                        node: start,
                        env,
                        assigned,
                    });
                }
                Node::ForEach {
                    var,
                    collection,
                    body,
                    ..
                } => {
                    let mut body_env = env.clone();
                    body_env.remove(var);
                    pending.push(Task::Stmts {
                        stmts: body,
                        index: 0,
                        env: body_env,
                        assigned: assigned.clone(),
                    });
                    pending.push(Task::Node {
                        node: collection,
                        env,
                        assigned,
                    });
                }
                Node::While { cond, body, .. } => {
                    pending.push(Task::Stmts {
                        stmts: body,
                        index: 0,
                        env: env.clone(),
                        assigned: assigned.clone(),
                    });
                    pending.push(Task::Node {
                        node: cond,
                        env,
                        assigned,
                    });
                }
                Node::Match {
                    scrutinee, arms, ..
                } => {
                    for arm in arms.iter().rev() {
                        let mut arm_env = env.clone();
                        remove_pattern_bindings(&arm.pattern, &mut arm_env);
                        pending.push(Task::Node {
                            node: &arm.body,
                            env: arm_env.clone(),
                            assigned: assigned.clone(),
                        });
                        if let Some(guard) = &arm.guard {
                            pending.push(Task::Node {
                                node: guard,
                                env: arm_env,
                                assigned: assigned.clone(),
                            });
                        }
                    }
                    pending.push(Task::Node {
                        node: scrutinee,
                        env,
                        assigned,
                    });
                }
                Node::Binary {
                    op, left, right, ..
                } => {
                    if matches!(op, BinOp::Eq | BinOp::Ne)
                        && is_string_value(left, &env)
                        && is_string_value(right, &env)
                    {
                        targets.insert(ordinals[&(node as *const Node as usize)]);
                    }
                    let children = super::closures::node_children_ref(node);
                    for child in children.into_iter().rev() {
                        pending.push(Task::Node {
                            node: child,
                            env: env.clone(),
                            assigned: assigned.clone(),
                        });
                    }
                }
                Node::Closure(data, _) => {
                    let closure_assigned = Rc::new(assigned_in(&data.body));
                    let closure_env = data
                        .captures
                        .iter()
                        .filter(|name| env.contains(*name) && !closure_assigned.contains(*name))
                        .cloned()
                        .collect();
                    pending.push(Task::Stmts {
                        stmts: &data.body,
                        index: 0,
                        env: closure_env,
                        assigned: closure_assigned,
                    });
                }
                Node::FnDef(fd, _) => push_stmts(&mut pending, &fd.body, StringEnv::new()),
                _ => {
                    let children = super::closures::node_children_ref(node);
                    for child in children.into_iter().rev() {
                        pending.push(Task::Node {
                            node: child,
                            env: env.clone(),
                            assigned: assigned.clone(),
                        });
                    }
                }
            },
        }
    }
    targets
}

/// Rewrite in deterministic preorder on an explicit heap worklist. A target is
/// replaced before its original operands are queued, so the synthesized `!= 0`
/// comparison is not counted as another source equality.
fn rewrite(module: &mut Module, targets: &mut Targets) {
    let mut next_equality = 0;
    let mut pending: Vec<&mut Node> = module.items.iter_mut().rev().collect();
    while let Some(node) = pending.pop() {
        let target = if matches!(
            node,
            Node::Binary {
                op: BinOp::Eq | BinOp::Ne,
                ..
            }
        ) {
            let ordinal = next_equality;
            next_equality += 1;
            targets.remove(&ordinal)
        } else {
            false
        };
        if target {
            let old = std::mem::replace(node, Node::Lit(Literal::Int(0), node.span()));
            let Node::Binary {
                op,
                left,
                right,
                span,
            } = old
            else {
                unreachable!("target ordinal did not name an equality node")
            };
            let eq = Node::Call {
                callee: "__mind_string_eq".to_string(),
                args: vec![*left, *right],
                span,
            };
            *node = if op == BinOp::Eq {
                eq
            } else {
                Node::Binary {
                    op: BinOp::Eq,
                    left: Box::new(eq),
                    right: Box::new(Node::Lit(Literal::Int(0), span)),
                    span,
                }
            };
        }
        let children = children_mut(node);
        pending.extend(children.into_iter().rev());
    }
}

/// Return a rewritten clone only when a proven comparison exists. The analysis
/// itself is iterative: modules with no target keep lower_expr's stack shape,
/// emitted bytes, and the Windows debug-stack margin unchanged.
pub(crate) fn normalize(module: &Module) -> Option<Module> {
    let ordinals = equality_ordinals(module);
    if ordinals.is_empty() {
        return None;
    }
    let mut targets = collect_targets(module, &ordinals);
    if targets.is_empty() {
        return None;
    }
    let mut rewritten = module.clone();
    rewrite(&mut rewritten, &mut targets);
    assert!(
        targets.is_empty(),
        "string equality normalization lost an analyzed AST target"
    );
    Some(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Span;

    fn count_calls(node: &Node, name: &str) -> usize {
        let here = matches!(node, Node::Call { callee, .. } if callee == name) as usize;
        here + match node {
            Node::FnDef(fd, _) => fd.body.iter().map(|n| count_calls(n, name)).sum::<usize>(),
            _ => super::super::closures::node_children_ref(node)
                .into_iter()
                .map(|n| count_calls(n, name))
                .sum::<usize>(),
        }
    }

    fn set_comparison_span(node: &mut Node, replacement: Span) {
        if let Node::Binary {
            op: BinOp::Eq | BinOp::Ne,
            span,
            ..
        } = node
        {
            *span = replacement;
        }
        for child in super::super::closures::node_children_mut(node) {
            set_comparison_span(child, replacement);
        }
    }

    #[test]
    fn rewrites_only_locally_proven_strings() {
        let module = crate::parser::parse(
            r#"fn same() -> i64 {
                let a: string = "a"
                let b: string = "b"
                let local: string = "x"
                if a == b { return 1 }
                if local != "y" { return 2 }
                return 0
            }"#,
        )
        .unwrap();
        let rewritten = normalize(&module).expect("two proven comparisons");
        assert_eq!(
            rewritten
                .items
                .iter()
                .map(|n| count_calls(n, "__mind_string_eq"))
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn numeric_and_invalidated_facts_are_no_op() {
        let module = crate::parser::parse(
            r#"fn control() -> i64 {
                let value: string = "x"
                value = 7
                if value == "x" { return 1 }
                if 2 != 3 { return 2 }
                return 0
            }"#,
        )
        .unwrap();
        assert!(normalize(&module).is_none());
    }

    #[test]
    fn identical_cross_source_spans_do_not_rewrite_numeric_comparisons() {
        let mut numeric = crate::parser::parse(
            "fn numeric(a: i64, b: i64) -> i64 { if a == b { return 1 }; return 0 }",
        )
        .unwrap();
        let mut textual = crate::parser::parse(
            "fn textual() -> i64 { let a = \"a\"; let b = \"b\"; if a == b { return 1 }; return 0 }",
        )
        .unwrap();
        let shared = Span::new(40, 46);
        set_comparison_span(&mut numeric.items[0], shared);
        set_comparison_span(&mut textual.items[0], shared);
        numeric.items.extend(textual.items);

        let rewritten = normalize(&numeric).expect("the string comparison is proven");
        assert_eq!(count_calls(&rewritten.items[0], "__mind_string_eq"), 0);
        assert_eq!(count_calls(&rewritten.items[1], "__mind_string_eq"), 1);
    }

    #[test]
    fn an_assigned_name_is_never_treated_as_immutable_string_proof() {
        let module = crate::parser::parse(
            r#"fn control(flag: bool) -> i64 {
                let value: string = "x"
                if flag { value = 7; let value: string = "x" }
                if value == "x" { return 1 }
                return 0
            }"#,
        )
        .unwrap();
        assert!(normalize(&module).is_none());
    }

    #[test]
    fn annotation_and_mixed_operands_do_not_create_string_proof() {
        let module = crate::parser::parse(
            r#"fn control(real: string) -> i64 {
                let fake: string = 7
                if fake == "x" { return 1 }
                if real == 7 { return 2 }
                if real == "x" { return 3 }
                return 0
            }"#,
        )
        .unwrap();
        assert!(normalize(&module).is_none());
    }

    #[test]
    fn lexical_shadowing_does_not_change_outer_proof() {
        let module = crate::parser::parse(
            r#"fn control() -> i64 {
                let value: string = "outer"
                if true { let value = 7 }
                if value == "outer" { return 1 }
                return 0
            }"#,
        )
        .unwrap();
        let rewritten = normalize(&module).expect("outer immutable string remains proven");
        assert_eq!(count_calls(&rewritten.items[0], "__mind_string_eq"), 1);
    }
}
