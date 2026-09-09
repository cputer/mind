// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

#![cfg(feature = "std-surface")]

use libmind::{parser, type_checker};

fn codes(source: &str) -> Vec<String> {
    let module = parser::parse(source).expect("narrow assignment fixture must parse");
    type_checker::check_module_types(&module, source, &Default::default())
        .into_iter()
        .map(|diagnostic| diagnostic.code.to_string())
        .collect()
}

#[test]
fn narrow_array_elements_refuse_proven_opaque_handles() {
    for source in [
        "struct S { xs: [u8; 1] }\nfn f(s: S) -> i64 { s.xs[0] = \"x\"; return s.xs[0] }\n",
        "type Byte = u8\nstruct S { xs: [Byte; 1] }\nfn f(s: S) -> i64 { let text = \"x\"; s.xs[0] = text; return s.xs[0] }\n",
        "struct V { x: i64 }\nstruct S { xs: [i16; 1] }\nfn f(s: S) -> i64 { s.xs[0] = V { x: 7 }; return s.xs[0] }\n",
    ] {
        let got = codes(source);
        assert_eq!(
            got.iter().filter(|code| code.as_str() == "E2036").count(),
            1,
            "expected one opaque-handle refusal for {source:?}: {got:?}"
        );
    }
}

#[test]
fn narrow_array_elements_preserve_integer_assignment_semantics() {
    let source = "type Word = u16\nstruct S { xs: [Word; 2] }\nfn f(s: S, x: i64) -> i64 { s.xs[0] = 65535; s.xs[1] = x + 1; return s.xs[0] }\n";
    let got = codes(source);
    assert!(
        !got.iter().any(|code| code == "E2036"),
        "integer assignments must remain admitted: {got:?}"
    );
}
