use super::*;

use crate::parser;
use std::collections::HashMap;

#[test]
fn eval_field_assignment_refuses_instead_of_returning_rhs() {
    let src = r#"
struct Point { x: i64 }

fn mutate() -> i64 {
    let mut p: Point = Point { x: 1 };
    p.x = 9;
    return p.x;
}

mutate()
"#;
    let module = parser::parse(src).expect("field-assignment fixture parses");
    let mut env = HashMap::new();
    let err = eval_module_value_with_env(&module, &mut env, Some(src)).unwrap_err();
    assert_eq!(
        err.to_string(),
        "unsupported: struct field assignment `.x` is unsupported by the interpreter; mutation semantics are not defined"
    );
}

#[test]
fn eval_module_level_field_assignment_refuses() {
    let src = r#"
struct Point { x: i64 }
let mut p: Point = Point { x: 1 };
p.x = 9;
"#;
    let module = parser::parse(src).expect("module field-assignment fixture parses");
    let mut env = HashMap::new();
    // This test targets the module executor's dispatch. Its hand-built
    // annotation is outside the evaluator's source type-check surface;
    // bypass that unrelated gate so the FieldAssign refusal is exercised.
    let err = eval_module_value_with_env(&module, &mut env, None).unwrap_err();
    assert!(
        err.to_string()
            .contains("struct field assignment `.x` is unsupported by the interpreter"),
        "unexpected refusal: {err}"
    );
}
