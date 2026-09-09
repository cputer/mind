#![cfg(feature = "cross-module-imports")]

use std::fs;

use libmind as mind;
use mind::ir::{IRModule, Instr, verify_canonical_metadata};
use mind::pipeline::compile_source_to_canonical_ir;
use mind::project::canonical_bridge::CanonicalBindingPlan;
use mind::project::single_file_scope::capture_project_scope;

fn project(
    source: &str,
    siblings: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    mind::project::single_file_scope::ProjectScope,
) {
    let root = tempfile::tempdir().expect("temporary project");
    let entry = root.path().join("main.mind");
    fs::write(&entry, source).expect("entry source");
    let mut candidates = vec![entry.clone()];
    for (name, text) in siblings {
        let path = root.path().join(name);
        fs::write(&path, text).expect("sibling source");
        candidates.push(path);
    }
    let scope = capture_project_scope(&entry, source, &candidates, root.path())
        .expect("captured project scope");
    (root, entry, scope)
}

fn compile(source: &str, siblings: &[(&str, &str)]) -> Result<mind::ir::IRModule, String> {
    let (_root, entry, scope) = project(source, siblings);
    compile_source_to_canonical_ir(source, Some(entry.to_str().unwrap()), &scope, "crate")
        .map_err(|error| error.to_string())
}

fn find_call(instrs: &[mind::ir::Instr]) -> Option<&mind::ir::Instr> {
    for instr in instrs {
        match instr {
            mind::ir::Instr::Call { .. } => return Some(instr),
            mind::ir::Instr::FnDef { body, .. } => {
                if let Some(call) = find_call(body) {
                    return Some(call);
                }
            }
            #[cfg(feature = "std-surface")]
            mind::ir::Instr::If {
                cond_instrs,
                then_instrs,
                else_instrs,
                ..
            } => {
                for branch in [cond_instrs, then_instrs, else_instrs] {
                    if let Some(call) = find_call(branch) {
                        return Some(call);
                    }
                }
            }
            #[cfg(feature = "std-surface")]
            mind::ir::Instr::While {
                cond_instrs, body, ..
            } => {
                for branch in [cond_instrs, body] {
                    if let Some(call) = find_call(branch) {
                        return Some(call);
                    }
                }
            }
            #[cfg(feature = "std-surface")]
            mind::ir::Instr::Region { body, .. } => {
                if let Some(call) = find_call(body) {
                    return Some(call);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn local_scalar_calls_are_co_located_and_verified() {
    let source = "fn inc(x: i64) -> i64 { return x; }\nfn main() -> i64 { return inc(1); }\n";
    let ir = compile(source, &[]).expect("canonical scalar source");
    let bundle = ir.canonical_types.as_ref().expect("canonical bundle");
    assert_eq!(bundle.functions().len(), 2);
    let Some(mind::ir::Instr::Call {
        resolved_callee: Some(identity),
        ..
    }) = find_call(&ir.instrs)
    else {
        panic!("canonical call identity was not emitted")
    };
    assert_eq!(identity.owner(), "crate");
    assert_eq!(identity.name(), "inc");
    let function_count = ir
        .instrs
        .iter()
        .filter(|instr| {
            matches!(
                instr,
                mind::ir::Instr::FnDef {
                    semantic_types: Some(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(function_count, 2);
}

#[test]
fn function_local_value_ids_are_scoped_independently() {
    let source = "fn first(x: i64) -> i64 { return x; }\nfn second(y: i64) -> i64 { return y; }\nfn main() -> i64 { return first(second(1)); }\n";
    let ir = compile(source, &[]).expect("scoped canonical scalar source");
    let mut parameter_zero_count = 0;
    for instr in &ir.instrs {
        let mind::ir::Instr::FnDef {
            name,
            semantic_types: Some(semantic),
            ..
        } = instr
        else {
            continue;
        };
        if matches!(name.as_str(), "first" | "second") {
            assert!(semantic.values().contains_key(&mind::ir::ValueId(0)));
            parameter_zero_count += 1;
        }
    }
    assert_eq!(parameter_zero_count, 2);
}

#[test]
fn branch_calls_are_bound_at_their_original_ast_spans() {
    let source = "fn inc(x: i64) -> i64 { return x; }\nfn main(x: i64) -> i64 { if x { return inc(x); } return x; }\n";
    let ir = compile(source, &[]).expect("branch call canonical source");
    assert!(find_call(&ir.instrs).is_some());
}

#[test]
fn scalar_branch_returns_remain_valid_when_each_path_has_a_value() {
    let source = "fn main(x: i64) -> i64 { if x { return 1; } return x; }\n";
    compile(source, &[]).expect("each scalar branch return has a value");
}

#[test]
fn typed_branch_return_without_a_value_is_refused_before_lowering() {
    let source = "fn main(x: i64) -> i64 { if x { return; } return 1; }\n";
    let error = compile(source, &[]).expect_err("typed branch return must carry a value");
    assert!(error.contains("return type mismatch"), "{error}");
    assert!(error.contains("no value"), "{error}");
}

#[test]
fn checked_scalar_arithmetic_call_and_return_are_proven_by_the_checker() {
    let source =
        "fn add(x: i64) -> i64 { return x + x; }\nfn main() -> i64 { return add(1 + 2); }\n";
    let ir = compile(source, &[]).expect("checked scalar arithmetic source");
    assert!(find_call(&ir.instrs).is_some());
}

#[test]
fn comparison_placeholder_is_unknown_and_refused_as_a_producer() {
    let source =
        "fn take(x: i64) -> i64 { return x; }\nfn main() -> i64 { return take(1 == 1); }\n";
    let error = compile(source, &[]).expect_err("comparison producer must refuse");
    assert!(error.contains("checked producer"), "{error}");
}

#[test]
fn every_explicit_branch_return_requires_a_checked_producer() {
    let source = "fn main(x: i64) -> i64 { if x { return 1 == 1; } return 1; }\n";
    let error = compile(source, &[]).expect_err("branch comparison producer must refuse");
    assert!(error.contains("checked producer"), "{error}");
}

#[test]
fn qualified_import_uses_captured_defining_owner() {
    let source = "import left;\nfn main() -> i64 { return left.pick(1); }\n";
    let ir = compile(
        source,
        &[("left.mind", "pub fn pick(x: i64) -> i64 { return x; }\n")],
    )
    .expect("qualified imported scalar source");
    let Some(mind::ir::Instr::Call {
        resolved_callee: Some(identity),
        ..
    }) = find_call(&ir.instrs)
    else {
        panic!("qualified call identity was not emitted")
    };
    assert_eq!(identity.owner(), "crate.left");
    assert_eq!(identity.name(), "pick");
}

#[test]
fn ambiguous_bare_import_is_refused_before_lowering() {
    let source = "import left;\nimport right;\nfn main() -> i64 { return pick(1); }\n";
    let (_root, _entry, scope) = project(
        source,
        &[
            ("left.mind", "pub fn pick(x: i64) -> i64 { return x; }\n"),
            ("right.mind", "pub fn pick(x: i64) -> i64 { return x; }\n"),
        ],
    );
    let parsed = mind::parser::parse(source).expect("ambiguous source parses");
    let plan_error = CanonicalBindingPlan::build(&parsed, source, &scope, "crate")
        .expect_err("canonical resolver must preserve ambiguity");
    assert!(plan_error.to_string().contains("ambiguous function"));
    let error = compile(
        source,
        &[
            ("left.mind", "pub fn pick(x: i64) -> i64 { return x; }\n"),
            ("right.mind", "pub fn pick(x: i64) -> i64 { return x; }\n"),
        ],
    )
    .expect_err("ambiguous bare import must refuse");
    assert!(error.contains("pick"), "{error}");
}

#[test]
fn generic_source_function_is_typed_refusal() {
    let source = "fn id<T>(x: i64) -> i64 { return x; }\nfn main() -> i64 { return id(1); }\n";
    let error = compile(source, &[]).expect_err("generic source call must refuse");
    assert!(error.contains("unsupported function authority"), "{error}");
}

#[test]
fn local_generic_shadow_does_not_fall_through_to_imported_scalar() {
    let source = "import dep;\nfn pick<T>(x: i64) -> i64 { return x; }\nfn main() -> i64 { return pick(1); }\n";
    let error = compile(
        source,
        &[("dep.mind", "pub fn pick(x: i64) -> i64 { return x; }\n")],
    )
    .expect_err("unsupported local shadow must not use imported function");
    assert!(error.contains("unsupported"), "{error}");
}

#[test]
fn local_scalar_alias_is_resolved_before_canonical_registration() {
    let source = "type Count = i64;\nfn echo(x: Count) -> Count { return x; }\nfn main() -> i64 { return echo(1); }\n";
    let ir = compile(source, &[]).expect("monomorphic scalar alias");
    assert_eq!(
        ir.canonical_types
            .expect("canonical bundle")
            .functions()
            .len(),
        2
    );
}

#[test]
fn legacy_frontend_does_not_populate_canonical_metadata() {
    let source = "fn main() -> i64 { return 7; }\n";
    let (_root, entry, scope) = project(source, &[]);
    let ordinary =
        mind::pipeline::compile_source(source, &mind::pipeline::CompileOptions::default())
            .expect("legacy compile");
    assert!(ordinary.ir.canonical_types.is_none());
    let canonical =
        compile_source_to_canonical_ir(source, Some(entry.to_str().unwrap()), &scope, "crate")
            .expect("opt-in compile");
    assert!(canonical.canonical_types.is_some());
}

#[test]
fn non_scalar_signature_is_refused_without_an_artifact() {
    let source = "fn bad(x: [i64; 2]) -> i64 { return 0; }\nfn main() -> i64 { return 0; }\n";
    let error = compile(source, &[]).expect_err("aggregate source signature must refuse");
    assert!(error.contains("non-scalar function annotation"), "{error}");
}

#[test]
fn unknown_aggregate_call_argument_is_refused_without_producer_metadata() {
    let source = "fn take(x: i64) -> i64 { return x; }\nfn main() -> i64 { return take([1]); }\n";
    let error = compile(source, &[]).expect_err("aggregate producer must refuse");
    assert!(error.contains("producer type"), "{error}");
}

#[test]
fn unknown_aggregate_return_is_refused_without_producer_metadata() {
    let source = "fn main() -> i64 { return [1]; }\n";
    let error = compile(source, &[]).expect_err("aggregate return producer must refuse");
    assert!(error.contains("producer type"), "{error}");
}

#[test]
fn unit_function_is_refused_before_canonical_lowering() {
    let source = "fn unit() { 1 }\nfn main() -> i64 { return 0; }\n";
    let error = compile(source, &[]).expect_err("unit function must refuse");
    assert!(error.contains("unit/implicit-return"), "{error}");
}

#[test]
fn explicit_empty_tuple_unit_function_is_refused_before_canonical_lowering() {
    let source = "fn unit() -> () { return; }\nfn main() -> i64 { return 0; }\n";
    let error = compile(source, &[]).expect_err("explicit unit function must refuse");
    assert!(error.contains("unit/implicit-return"), "{error}");
}

#[test]
fn legacy_return_without_a_value_remains_unchecked_without_authority() {
    let mut module = IRModule::new();
    module.instrs.push(Instr::Return { value: None });
    assert_eq!(verify_canonical_metadata(&module), Ok(()));
}

#[test]
fn generated_gradient_is_refused_before_lowering() {
    let source = "let x: Tensor[f32,(2,3)] = 0; grad(tensor.sum(x), wrt=[x])\n";
    let error = compile(source, &[]).expect_err("gradient form must refuse");
    assert!(error.contains("gradient"), "{error}");
}

#[test]
fn external_constant_is_refused_before_lowering() {
    let source = "extern const TABLE: [i64; 1]\nfn main() -> i64 { return 0; }\n";
    let error = compile(source, &[]).expect_err("external constant must refuse");
    assert!(error.contains("external constants"), "{error}");
}
