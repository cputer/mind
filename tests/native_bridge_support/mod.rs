//! Shared harness for the native module-bridge integration controls.
//!
//! Split out so neither control file exceeds the 800-line ceiling
//! `tests/module_size_ratchet.rs` pins. This is a `tests/<dir>/mod.rs` module,
//! not a test target, so it compiles into each consumer rather than running as
//! its own binary. Nothing here was rewritten: the harness moved verbatim and
//! only gained the `pub` needed to cross the module boundary.

#![allow(dead_code)]

// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

use std::path::{Path, PathBuf};
use std::process::Command;

pub const MINDC: &str = env!("CARGO_BIN_EXE_mindc");

pub struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    /// A project with a manifest and a git root, which the bounded project-root
    /// scan requires before it will resolve siblings.
    pub fn new(name: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(
            root.join("Mind.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n\
                 [build]\ntarget = \"cpu\"\nemit = \"binary\"\nentry = \"src/main.mind\"\n"
            ),
        )
        .expect("manifest");
        // `find_project_root_for_file_checked` is git-root bounded.
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(root)
                .status()
                .expect("git init")
                .success(),
            "git init must succeed or every case degrades to MissingProject"
        );
        Self { dir }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn write(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.root().join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&p, body).expect("write");
        p
    }

    /// Build through the native backend. Returns (exit code, stderr, artifact bytes).
    pub fn build_native(&self, entry_rel: &str) -> (i32, String, Option<Vec<u8>>) {
        self.build_native_emit(entry_rel, "binary")
    }

    /// Build with NO source argument: the manifest entry must drive it.
    pub fn build_native_no_path(&self) -> (i32, String, Option<Vec<u8>>) {
        self.build_native_emit("", "binary")
    }

    pub fn build_native_emit(&self, entry_rel: &str, emit: &str) -> (i32, String, Option<Vec<u8>>) {
        let out = self.root().join("out.elf");
        let _ = std::fs::remove_file(&out);
        let mut args: Vec<String> = vec!["build".into()];
        if !entry_rel.is_empty() {
            args.push(entry_rel.into());
        }
        let res0 = args;
        let res = Command::new(MINDC)
            .args(res0)
            .args([
                "--release".to_string(),
                "--backend".to_string(),
                "native".to_string(),
                format!("--emit={emit}"),
                "--out".to_string(),
            ])
            .arg(&out)
            .current_dir(self.root())
            .env("MINDC_STD_DIR", repo_path("std"))
            .env(
                "MINDC_NATIVE_ELF",
                repo_path("examples/mindc_mind/testdata/selfhost_loop/stage1.elf"),
            )
            .output()
            .expect("spawn mindc");
        let bytes = std::fs::read(&out).ok();
        (
            res.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&res.stderr).into_owned(),
            bytes,
        )
    }
}

pub fn repo_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Run an emitted artifact and return its exit code.
pub fn run(bytes: &[u8]) -> i32 {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().join("run.elf");
    std::fs::write(&p, bytes).expect("write elf");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&p).expect("meta").permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&p, perm).expect("chmod");
    }
    Command::new(&p)
        .status()
        .expect("run artifact")
        .code()
        .unwrap_or(-1)
}
