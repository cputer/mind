// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Manifest-captured import ownership must be shared by check and emit.

#![cfg(all(
    unix,
    feature = "mlir-build",
    feature = "std-surface",
    feature = "cross-module-imports"
))]

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

fn manifest(entry: &str, output: &str, export: &str) -> String {
    format!(
        "[package]\nname = \"{output}\"\nversion = \"0.1.0\"\n\n\
         [build]\nentry = \"{entry}\"\noutput = \"{output}\"\n\n\
         [targets.cpu]\nbackend = \"cpu\"\n\n\
         [exports]\nc_abi = [\"{export}\"]\n"
    )
}

fn check(mindc: &Path, root: &Path, source: &str) -> std::process::Output {
    Command::new(mindc)
        .current_dir(root)
        .args(["check", source])
        .output()
        .expect("run mindc check")
}

fn emit_and_read(mindc: &Path, root: &Path, source: &str, output: &Path) {
    let out = Command::new(mindc)
        .current_dir(root)
        .args([
            "--emit-shared",
            output.to_str().expect("shared output path"),
            source,
        ])
        .output()
        .expect("run mindc emit-shared");
    assert!(
        out.status.success() && output.is_file(),
        "captured scope emit failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let py = format!(
        "import ctypes\n\
         lib = ctypes.CDLL(r'{}')\n\
         class Widget(ctypes.Structure):\n\
         \x20   _fields_ = [('value', ctypes.c_int64)]\n\
         f = lib.read\n\
         f.argtypes = [ctypes.POINTER(Widget)]\n\
         f.restype = ctypes.c_int64\n\
         assert f(ctypes.byref(Widget(42))) == 42\n\
         print('ok')\n",
        output.to_string_lossy()
    );
    let run = Command::new("python3")
        .args(["-c", &py])
        .output()
        .expect("run emitted shared library");
    assert!(
        run.status.success(),
        "captured scope artifact returned the wrong value:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn nested_project_import_scope_controls() {
    let mindc = common::require_mindc();
    let temp = tempfile::tempdir().expect("nested import scope scratch");

    // The manifest entry is at the project root while its imported sibling is
    // under src/. Captured resolution must preserve crate.src.types_a.
    let root_entry = temp.path().join("root_entry");
    fs::create_dir_all(root_entry.join("src")).expect("root-entry src");
    fs::write(
        root_entry.join("Mind.toml"),
        manifest("main.mind", "root_entry_scope", "read"),
    )
    .expect("root-entry manifest");
    fs::write(
        root_entry.join("main.mind"),
        "fn main() -> i64 {\n    return 0;\n}\n",
    )
    .expect("root-entry main");
    fs::write(
        root_entry.join("src/types_a.mind"),
        "struct Widget {\n    value: i64,\n}\n",
    )
    .expect("root-entry sibling");
    fs::write(
        root_entry.join("src/consumer.mind"),
        "import types_a;\nfn read(w: types_a.Widget) -> i64 {\n    return w.value;\n}\n",
    )
    .expect("root-entry consumer");
    let out = check(&mindc, &root_entry, "src/consumer.mind");
    assert!(
        out.status.success(),
        "captured root-entry scope check failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    emit_and_read(
        &mindc,
        &root_entry,
        "src/consumer.mind",
        &root_entry.join("root-entry.so"),
    );

    // Existing src-entry projects retain their exact crate.types_a path.
    let src_entry = temp.path().join("src_entry");
    fs::create_dir_all(src_entry.join("src")).expect("src-entry src");
    fs::write(
        src_entry.join("Mind.toml"),
        manifest("src/main.mind", "src_entry_scope", "read"),
    )
    .expect("src-entry manifest");
    fs::write(
        src_entry.join("src/main.mind"),
        "fn main() -> i64 {\n    return 0;\n}\n",
    )
    .expect("src-entry main");
    fs::write(
        src_entry.join("src/types_a.mind"),
        "struct Widget {\n    value: i64,\n}\n",
    )
    .expect("src-entry sibling");
    fs::write(
        src_entry.join("src/consumer.mind"),
        "import types_a;\nfn read(w: types_a.Widget) -> i64 {\n    return w.value;\n}\n",
    )
    .expect("src-entry consumer");
    let out = check(&mindc, &src_entry, "src/consumer.mind");
    assert!(
        out.status.success(),
        "src-entry scope control failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Duplicate stems are not resolved by first match. The qualified type
    // must fail closed when the captured owner is ambiguous.
    let ambiguous = temp.path().join("ambiguous");
    fs::create_dir_all(ambiguous.join("src/a")).expect("ambiguous a");
    fs::create_dir_all(ambiguous.join("src/b")).expect("ambiguous b");
    fs::write(
        ambiguous.join("Mind.toml"),
        manifest("main.mind", "ambiguous_scope", "read"),
    )
    .expect("ambiguous manifest");
    fs::write(
        ambiguous.join("main.mind"),
        "fn main() -> i64 {\n    return 0;\n}\n",
    )
    .expect("ambiguous main");
    for path in ["src/a/types_a.mind", "src/b/types_a.mind"] {
        fs::write(
            ambiguous.join(path),
            "struct Widget {\n    value: i64,\n}\n",
        )
        .expect("ambiguous sibling");
    }
    fs::write(
        ambiguous.join("src/consumer.mind"),
        "import types_a;\nfn read(w: types_a.Widget) -> i64 {\n    return w.value;\n}\n",
    )
    .expect("ambiguous consumer");
    let out = check(&mindc, &ambiguous, "src/consumer.mind");
    let rendered = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "ambiguous stem compiled: {rendered}");
    assert!(
        rendered.contains("E2002"),
        "ambiguous stem lost E2002: {rendered}"
    );
    assert!(
        !rendered.contains("panicked at"),
        "ambiguous stem panicked: {rendered}"
    );

    // An exact owner can be selected while a non-exported type remains
    // inaccessible to the consumer.
    let private = temp.path().join("private");
    fs::create_dir_all(private.join("src")).expect("private src");
    fs::write(
        private.join("Mind.toml"),
        manifest("main.mind", "private_scope", "read"),
    )
    .expect("private manifest");
    fs::write(
        private.join("main.mind"),
        "fn main() -> i64 {\n    return 0;\n}\n",
    )
    .expect("private main");
    fs::write(
        private.join("src/types_a.mind"),
        "export { helper }\nstruct Widget {\n    value: i64,\n}\nfn helper() -> i64 {\n    return 0;\n}\n",
    )
    .expect("private sibling");
    fs::write(
        private.join("src/consumer.mind"),
        "import types_a;\nfn read(w: types_a.Widget) -> i64 {\n    return w.value;\n}\n",
    )
    .expect("private consumer");
    let out = check(&mindc, &private, "src/consumer.mind");
    let rendered = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.status.success(), "private type compiled: {rendered}");
    assert!(
        rendered.contains("E2002"),
        "private type lost E2002: {rendered}"
    );
    assert!(
        !rendered.contains("panicked at"),
        "private type panicked: {rendered}"
    );
}
