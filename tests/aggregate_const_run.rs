// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Aggregate constants and dynamically constructed fixed arrays must preserve
//! their element values through executable MLIR lowering.

#![cfg(all(unix, feature = "mlir-build", feature = "std-surface"))]

mod common;

use libloading::{Library, Symbol};
use std::process::Command;

const SOURCE: &str = r#"
struct Pair { a: i64, b: i64 }
struct Wrapper { pair: Pair }
struct Encoding { w0: i64, w1: i64 }
struct Item { value: i64, tag: i64 }
struct Bag { xs: [i64; 1] }
type Items = [Item; 2]

const ITEM: Pair = Pair { a: 11, b: 13 };
const ITEMS: [Pair; 2] = [Pair { a: 11, b: 13 }, Pair { a: 17, b: 19 }];
const WRAPPERS: [Wrapper; 1] = [Wrapper { pair: Pair { a: 23, b: 29 } }];
const SIGNED: [i64; 2] = [-7, 5];

fn struct_const() -> i64 { return ITEM.a; }
fn struct_array_const() -> i64 { return ITEMS[1].b; }
fn nested_struct_array_const() -> i64 { return WRAPPERS[0].pair.b; }
fn signed_const_control() -> i64 { return SIGNED[0]; }

fn declared_before_const() -> i64 { return LATE.b; }
const LATE: Pair = Pair { a: 31, b: 37 };

fn read_item(items: [Pair; 2]) -> i64 { return items[1].b; }
fn struct_array_param() -> i64 {
    return read_item([Pair { a: 11, b: 13 }, Pair { a: 17, b: 19 }]);
}

fn encode_items(items: [Pair; 2]) -> Encoding {
    return Encoding { w0: items[0].a, w1: items[1].b };
}

fn returned_struct_assert_receiver() -> i64 {
    let e = encode_items(ITEMS);
    assert e.w0 == 11, "w0";
    return e.w1;
}

fn make_items() -> [Item; 2] {
    return [Item { value: 17, tag: 3 }, Item { value: 42, tag: 9 }];
}

fn make_items_alias() -> Items {
    return [Item { value: 17, tag: 3 }, Item { value: 42, tag: 9 }];
}

// A declared full array return keeps its record element schema through the
// call. Exercise a second element and both fields, including the local binding
// path that reuses the same declared return type.
fn returned_record_array_fields() -> i64 {
    let items = make_items();
    return items[1].value + make_items()[0].tag;
}

fn returned_record_array_alias_fields() -> i64 {
    return make_items_alias()[1].value + make_items_alias()[0].tag;
}

fn parenthesized_array_receiver() -> i64 {
    return (ITEMS)[1].b;
}

fn array_alias_receiver() -> i64 {
    let alias = ITEMS;
    return alias[1].b;
}

fn lexical_array_shadow_control() -> i64 {
    let alias = ITEMS;
    if 1 == 1 {
        let alias: i64 = 7;
        assert alias == 7, "inner scalar shadow";
    }
    return alias[0].a;
}

fn dynamic_scalars(x: i64) -> i64 {
    let values: [i64; 3] = [x, x + 1, x + 2];
    return values[2];
}

fn dynamic_structs(x: i64) -> i64 {
    let values: [Pair; 2] = [Pair { a: x, b: x + 1 }, Pair { a: x + 2, b: x + 3 }];
    return values[1].b;
}

fn stamp(state: Pair, digit: i64) -> i64 {
    state.a = state.a * 10 + digit;
    return digit;
}

fn dynamic_evaluation_order() -> i64 {
    let mut state = Pair { a: 0, b: 0 };
    let values: [i64; 2] = [stamp(state, 1), stamp(state, 2)];
    return state.a * 100 + values[0] * 10 + values[1];
}

fn local_struct_control() -> i64 {
    let p = Pair { a: 11, b: 13 };
    return p.a;
}

fn set99(x: Item) -> i64 {
    x.value = 99;
    return x.value;
}

// Copying the fixed-array container preserves the identity of its record
// elements. The mutation is observed through the original container.
fn record_alias_identity() -> i64 {
    let original = Item { value: 42 };
    let a: [Item; 1] = [original];
    let b = a;
    set99(b[0]);
    return a[0].value;
}

// A struct value is identity-bearing, so copying a struct that owns a fixed
// array keeps field mutations visible through the original owner.
fn bag_alias_identity() -> i64 {
    let b = Bag { xs: [1] };
    let c = b;
    c.xs[0] = 5;
    return b.xs[0];
}

// Replacing an element changes the copied container only; it does not mutate
// the original record reference stored at the same position.
fn array_container_copy() -> i64 {
    let original = Item { value: 42 };
    let a: [Item; 1] = [original];
    let mut b = a;
    b[0] = Item { value: 99 };
    return a[0].value;
}
"#;

#[test]
fn aggregate_constants_and_dynamic_fixed_arrays_run() {
    let mindc = common::require_mindc();

    let dir = tempfile::tempdir().expect("aggregate scratch");
    let source = dir.path().join("main.mind");
    let shared = dir.path().join("aggregate-a.so");
    let repeated = dir.path().join("aggregate-b.so");
    std::fs::write(&source, SOURCE).expect("write aggregate source");
    for artifact in [&shared, &repeated] {
        let out = Command::new(&mindc)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .arg(&source)
            .arg("--emit-shared")
            .arg(artifact)
            .output()
            .expect("run standalone mindc");
        assert!(
            out.status.success() && artifact.is_file(),
            "aggregate compile failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(
        std::fs::read(&shared).expect("read first aggregate artifact"),
        std::fs::read(&repeated).expect("read repeated aggregate artifact"),
        "repeated aggregate compilation must be byte-identical"
    );

    unsafe {
        let lib = Library::new(&shared).expect("load aggregate shared library");
        let noarg = |name: &[u8]| -> i64 {
            let f: Symbol<unsafe extern "C" fn() -> i64> =
                lib.get(name).expect("load no-arg aggregate function");
            f()
        };
        let onearg = |name: &[u8], x: i64| -> i64 {
            let f: Symbol<unsafe extern "C" fn(i64) -> i64> =
                lib.get(name).expect("load aggregate function");
            f(x)
        };

        assert_eq!(noarg(b"struct_const"), 11);
        assert_eq!(noarg(b"struct_array_const"), 19);
        assert_eq!(noarg(b"nested_struct_array_const"), 29);
        assert_eq!(noarg(b"signed_const_control"), -7);
        assert_eq!(noarg(b"declared_before_const"), 37);
        assert_eq!(noarg(b"struct_array_param"), 19);
        assert_eq!(noarg(b"returned_struct_assert_receiver"), 19);
        assert_eq!(noarg(b"returned_record_array_fields"), 45);
        assert_eq!(noarg(b"returned_record_array_alias_fields"), 45);
        assert_eq!(noarg(b"parenthesized_array_receiver"), 19);
        assert_eq!(noarg(b"array_alias_receiver"), 19);
        assert_eq!(noarg(b"lexical_array_shadow_control"), 11);
        assert_eq!(onearg(b"dynamic_scalars", 40), 42);
        assert_eq!(onearg(b"dynamic_scalars", -3), -1);
        assert_eq!(onearg(b"dynamic_structs", 40), 43);
        assert_eq!(noarg(b"dynamic_evaluation_order"), 1212);
        assert_eq!(noarg(b"local_struct_control"), 11);
        assert_eq!(noarg(b"record_alias_identity"), 99);
        assert_eq!(noarg(b"bag_alias_identity"), 5);
        assert_eq!(noarg(b"array_container_copy"), 42);
    }
}
