// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Executing-test coverage for transparent top-level module blocks.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use libmind::test::discover_tests_in_source;

fn project(name: &str, source: &str) -> PathBuf {
    let root = common::scratch_dir("mindc-test-nested-modules").join(name);
    fs::create_dir_all(root.join(".git")).expect("create project boundary");
    fs::create_dir_all(root.join("src")).expect("create source dir");
    fs::create_dir_all(root.join("tests")).expect("create tests dir");
    fs::write(
        root.join("Mind.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"main.mind\"\n"
        ),
    )
    .expect("write manifest");
    fs::write(root.join("main.mind"), "fn main() -> i32 { return 0; }\n").expect("write entry");
    fs::write(root.join("tests/nested.mind"), source).expect("write test source");
    root
}

fn run(root: &Path, threads: usize) -> Output {
    Command::new(common::mindc_bin())
        .args([
            "test",
            "tests/nested.mind",
            "--threads",
            &threads.to_string(),
        ])
        .current_dir(root)
        .output()
        .expect("run mindc test")
}

fn text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn nested_false_assert_and_bool_are_executed_on_one_and_four_workers() {
    let root = project(
        "nested_failures_execute",
        r#"
let FIRST: i64 = 40;
let BASE: i64 = FIRST + 1;
let TABLE: [i64; 3] = [10, 20, 30];
fn get() -> i64 { return BASE; }
fn caller() -> i64 { let BASE: i64 = 99; return get(); }
fn table_value(index: i64) -> i64 { return TABLE[index]; }
module wrapper {
    #[test]
    fn false_assert() {
        assert false, "nested assertion executed";
    }

    #[test]
    fn false_bool() -> bool {
        return false;
    }

    #[test]
    fn module_binding() {
        assert get() == 41, "module let stays lexical";
        assert caller() == 41, "caller local does not leak";
        assert table_value(1) == 20, "fixed-array module table";
    }
}
"#,
    );
    for threads in [1, 4] {
        let output = run(&root, threads);
        let rendered = text(&output);
        assert!(!output.status.success(), "threads={threads}\n{rendered}");
        assert!(rendered.contains("running 3 tests"), "{rendered}");
        assert!(rendered.contains("nested assertion executed"), "{rendered}");
        assert!(rendered.contains("test returned false (0)"), "{rendered}");
        assert!(rendered.contains("1 passed; 2 failed"), "{rendered}");
    }
}

#[test]
#[cfg(feature = "std-surface")]
fn tensor_parameter_metadata_respects_shadowing_in_nested_tests() {
    let root = project(
        "nested_tensor_parameter_scope",
        r#"
let SHADOW: Tensor[f32,()] = 1;
let mut COUNT: i64 = 0;
fn count_inner() -> i64 { return COUNT; }
fn count() -> i64 { return count_inner(); }
while COUNT < 2 {
    COUNT = COUNT + 1;
    assert count() == COUNT, "loop observes updated module global";
}
fn scalar_shadow(SHADOW: i64) { grad(SHADOW, wrt=[SHADOW]); }
fn tensor_shadow(SHADOW: Tensor[f32,()]) { grad(SHADOW, wrt=[SHADOW]); }
module wrapper {
    #[test]
    fn scalar_parameter_removes_tensor_metadata() { scalar_shadow(7); }
    #[test]
    fn tensor_parameter_installs_tensor_metadata() { tensor_shadow(SHADOW); }
    #[test]
    fn module_loop_updates_callee_globals() { assert count() == 2, "loop mutation stays visible"; }
}
"#,
    );
    for threads in [1, 4] {
        let output = run(&root, threads);
        let rendered = text(&output);
        assert!(!output.status.success(), "threads={threads}\n{rendered}");
        assert!(rendered.contains("running 3 tests"), "{rendered}");
        assert!(rendered.contains("unknown tensor variable"), "{rendered}");
        assert!(rendered.contains("2 passed; 1 failed"), "{rendered}");
    }
}

#[test]
#[cfg(feature = "cross-module-imports")]
fn nested_helper_const_and_project_import_execute() {
    let root = project(
        "nested_helpers_execute",
        r#"
module wrapper {
    import dep;
    const LOCAL: i64 = 1;
    fn helper() -> i64 { return dep.value() + LOCAL; }

    #[test]
    fn nested_pass() {
        assert helper() == 42, "nested declarations and import";
    }
}
"#,
    );
    fs::write(
        root.join("src/dep.mind"),
        "export { value }\nfn value() -> i64 { return 41; }\n",
    )
    .expect("write dependency");
    let output = run(&root, 4);
    let rendered = text(&output);
    assert!(output.status.success(), "{rendered}");
    assert!(rendered.contains("1 passed; 0 failed"), "{rendered}");
}

#[test]
fn duplicate_test_name_is_ambiguous_even_when_other_definition_is_not_a_test() {
    let source = r#"
module first {
    #[test]
    fn repeated() { assert true; }
}
module second {
    fn repeated() { assert true; }
}
"#;
    let path = Path::new("duplicate_nested.mind");
    let error = discover_tests_in_source(path, source).expect_err("duplicate must be refused");
    assert!(
        error.contains("ambiguous test function `repeated`"),
        "{error}"
    );

    let root = project("nested_duplicate_refusal", source);
    let output = run(&root, 1);
    let rendered = text(&output);
    assert!(!output.status.success(), "{rendered}");
    assert!(rendered.contains("ambiguous test function"), "{rendered}");
    assert!(rendered.contains("no tests found"), "{rendered}");
}

#[test]
fn function_body_local_test_is_not_discovered() {
    let source = r#"
fn outer() {
    #[test]
    fn local_test() { assert false, "must stay local"; }
}
"#;
    let entries = discover_tests_in_source(Path::new("local_fn.mind"), source)
        .expect("nested local function parses");
    assert!(
        entries.is_empty(),
        "function-local test leaked into discovery"
    );

    let root = project("nested_local_test_excluded", source);
    let output = run(&root, 4);
    let rendered = text(&output);
    assert!(!output.status.success(), "{rendered}");
    assert!(rendered.contains("running 0 tests"), "{rendered}");
    assert!(rendered.contains("no tests found"), "{rendered}");
}

#[test]
fn nested_discovery_preserves_depth_first_source_order_and_lines() {
    let source = "module first {\n#[test]\nfn alpha() {}\n}\n\
                  #[test]\nfn middle() {}\n\
                  module last {\n#[test]\nfn omega() {}\n}\n";
    let entries = discover_tests_in_source(Path::new("order.mind"), source).expect("discover");
    let names = entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["order::alpha", "order::middle", "order::omega"]);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.source_line)
            .collect::<Vec<_>>(),
        [3, 6, 9]
    );
}
