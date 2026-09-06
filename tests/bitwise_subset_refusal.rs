//! The low-level subset must refuse bitwise syntax at the parser boundary.

#[cfg(not(feature = "std-surface"))]
#[test]
fn canon_cos_fixture_is_refused_without_std_surface() {
    let source = include_str!("../examples/dottie_collapse.mind");
    let qmul_start = source.find("fn qmul(").expect("shipped qmul body");
    let qmul_end = qmul_start
        + source[qmul_start..]
            .find("\n}\n")
            .expect("shipped qmul closing brace")
        + 3;
    let shift = qmul_start
        + source[qmul_start..qmul_end]
            .find(">>")
            .expect("qmul shift operator");
    let errors = libmind::parser::parse(source).expect_err("bitwise must be refused");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].cause_code.as_deref(), Some("E1042"));
    assert!(errors[0].message.contains("`>>`"));
    assert!(
        errors[0].offset >= shift && errors[0].offset < qmul_end,
        "refusal offset {} is outside qmul [{qmul_start}, {qmul_end})",
        errors[0].offset
    );

    let diagnostics = libmind::parser::parse_with_diagnostics(source)
        .expect_err("the pretty diagnostic adapter must preserve the cause code");
    assert_eq!(diagnostics[0].code, "E1042");
}
