// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0

//! The build-artifact NAME has exactly ONE owner.
//!
//! # The defect this pins
//!
//! `mindc build` resolved the default artifact name in two places that
//! disagreed, and neither matched the manifest documentation or RFC 0008 §3
//! (`output` — "output artifact name, without extension. Default:
//! `package.name`"):
//!
//! * `src/build/mod.rs` named it after `[package] name`, ignoring
//!   `[build] output` entirely — so a declared `output` was written by the
//!   compile path and then RENAMED away by the orchestrator, i.e. the field
//!   parsed, validated, and did nothing.
//! * `src/project/mod.rs` named it after `[build] output`, whose serde default
//!   was the literal `"app"` — a third spelling matching neither the other
//!   owner nor its own doc comment.
//!
//! Both spellings reach disk. On a host WITH the native backend the
//! orchestrator renames the compile path's file to its own spelling, so
//! `<package.name>` wins; on a host without one the build refuses (`E5003`)
//! BEFORE that rename, leaving the artifact under the compile path's spelling
//! (`app`). One manifest, two artifact names, decided by a host capability.
//!
//! That divergence is not cosmetic: a downstream harness must hand-type one of
//! the two rules to find the artifact, and picking the wrong one is how a
//! byte-identity check came to panic on exactly the tier it exists for.
//!
//! # What is asserted
//!
//! [`artifact_stem`] is the single resolver; the tests below pin its precedence
//! directly and pin that `mindc build` REPORTS and WRITES that one name on both
//! the native path and the launcher-refusal path.
//!
//! Gate: `cargo test --release --features "mlir-build std-surface cross-module-imports" --test mindc_artifact_name`

// ---------------------------------------------------------------------------
// End-to-end: the name `mindc build` reports is the name it wrote
// ---------------------------------------------------------------------------

#[cfg(feature = "mlir-build")]
mod common;

#[cfg(feature = "mlir-build")]
mod e2e {
    use crate::common::{gate, reported_artifact, require_mindc, run_build_captured};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Output;

    const HELLO_MIND: &str = "fn main() -> i64 { 42 }\n";

    /// Write a project rooted at `dir` and return its `target/debug` directory.
    fn write_project(dir: &Path, manifest: &str) -> PathBuf {
        // A scratch dir is pid-keyed, not run-keyed: clear it so a leftover from
        // an earlier run cannot supply the artifact name under test.
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir.join("src")).expect("create src");
        fs::write(dir.join("Mind.toml"), manifest).expect("write Mind.toml");
        fs::write(dir.join("src/main.mind"), HELLO_MIND).expect("write main.mind");
        dir.join("target").join("debug")
    }

    fn run_build(dir: &Path) -> Output {
        run_build_captured(&require_mindc(), dir, &[])
    }

    /// Every artifact-shaped file the build left in `target/debug`, sorted.
    ///
    /// The launcher-refusal path leaves its artifact here without reporting a
    /// path, so this is what proves the refusal used the SAME name — reading
    /// the directory rather than asserting a spelling this harness chose.
    fn artifacts_in(target_dir: &Path) -> Vec<String> {
        let Ok(entries) = fs::read_dir(target_dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with(".o") && !n.ends_with(".mlir") && !n.ends_with(".ll"))
            .collect();
        names.sort();
        names
    }

    /// A declared `[build] output` names the artifact `mindc build` reports AND
    /// writes. Before the single resolver the orchestrator reported
    /// `<package.name>` while the compile path wrote `<build.output>` and the
    /// orchestrator then renamed it away, so the declared field was inert.
    #[test]
    fn declared_build_output_names_the_reported_artifact() {
        let dir = crate::common::scratch_dir("mindc_artifact_name_declared");
        let target_dir = write_project(
            &dir,
            "[package]\nname = \"artifact_name_declared\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.mind\"\noutput = \"declared_artifact\"\n",
        );

        let out = run_build(&dir);
        if !gate::compiled("mindc_artifact_name", &out) {
            // Launcher-refusal path: no `Finished` line, but the artifact the
            // refusal left behind must carry the SAME declared name.
            assert_eq!(
                artifacts_in(&target_dir),
                vec!["declared_artifact".to_string()],
                "the refused build left an artifact under a name other than the \
                 declared [build] output"
            );
            return;
        }

        let reported = reported_artifact(&out);
        assert_eq!(
            reported.file_name().and_then(|n| n.to_str()),
            Some("declared_artifact"),
            "`mindc build` reported {} but [build] output declares \
             `declared_artifact`",
            reported.display()
        );
        assert!(
            reported.exists(),
            "`mindc build` reported {} but wrote no file there",
            reported.display()
        );
        assert_eq!(
            artifacts_in(&target_dir),
            vec!["declared_artifact".to_string()],
            "exactly one artifact name may reach disk"
        );
    }

    /// With no `[build] output`, the artifact is `package.name` on BOTH paths.
    ///
    /// The `"app"` serde default was the compile path's spelling, so it was the
    /// name the launcher-refusal path left on disk while the native path
    /// produced `<package.name>` from the same manifest.
    #[test]
    fn undeclared_output_defaults_to_package_name_on_both_paths() {
        let dir = crate::common::scratch_dir("mindc_artifact_name_default");
        let target_dir = write_project(
            &dir,
            "[package]\nname = \"artifact_name_default\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.mind\"\n",
        );

        let out = run_build(&dir);
        let compiled = gate::compiled("mindc_artifact_name", &out);

        // Holds on the native path and on the refusal path alike: `app` is not
        // a name any manifest here asked for.
        assert_eq!(
            artifacts_in(&target_dir),
            vec!["artifact_name_default".to_string()],
            "the artifact must be named after [package] name on both the native \
             path and the launcher-refusal path"
        );

        if !compiled {
            return;
        }
        let reported = reported_artifact(&out);
        assert_eq!(
            reported.file_name().and_then(|n| n.to_str()),
            Some("artifact_name_default"),
            "`mindc build` reported {}",
            reported.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Unit: the resolver's precedence, feature-independent
// ---------------------------------------------------------------------------

mod resolver {
    use libmind::project::{DEFAULT_TARGET_BLOCK, ProjectManifest, artifact_stem};

    fn manifest(toml_src: &str) -> ProjectManifest {
        toml::from_str(toml_src).expect("parse manifest")
    }

    /// The documented default (`Default: package.name`) is now the ACTUAL
    /// default. It was the literal `"app"`, a name no manifest asked for.
    #[test]
    fn undeclared_output_is_package_name() {
        let m = manifest("[package]\nname = \"my_pkg\"\nversion = \"0.1.0\"\n");
        assert_eq!(artifact_stem(&m, None), "my_pkg");
        assert_eq!(artifact_stem(&m, Some(DEFAULT_TARGET_BLOCK)), "my_pkg");
        assert_eq!(artifact_stem(&m, Some("no_such_block")), "my_pkg");
    }

    /// A declared `[build] output` wins over `package.name`. It used to be
    /// honoured by the compile path and then renamed away by the orchestrator,
    /// so the field parsed and did nothing.
    #[test]
    fn declared_build_output_wins_over_package_name() {
        let m = manifest(
            "[package]\nname = \"my_pkg\"\nversion = \"0.1.0\"\n\n\
             [build]\noutput = \"chosen\"\n",
        );
        assert_eq!(artifact_stem(&m, None), "chosen");
        assert_eq!(artifact_stem(&m, Some(DEFAULT_TARGET_BLOCK)), "chosen");
    }

    /// A block's own `output` wins for THAT block only; a block that declares
    /// none falls through to `[build] output`, then to `package.name`.
    #[test]
    fn target_block_output_wins_for_its_own_block() {
        let m = manifest(
            "[package]\nname = \"my_pkg\"\nversion = \"0.1.0\"\n\n\
             [build]\noutput = \"chosen\"\n\n\
             [targets.cpu]\nbackend = \"cpu\"\noutput = \"cpu_named\"\n\n\
             [targets.gpu]\nbackend = \"gpu\"\n",
        );
        assert_eq!(artifact_stem(&m, Some("cpu")), "cpu_named");
        assert_eq!(artifact_stem(&m, Some("gpu")), "chosen");
        // No target selected consults the same block `build_project` does.
        assert_eq!(artifact_stem(&m, None), "cpu_named");
    }

    /// The stem carries no directory and no extension: those belong to the
    /// emit-aware caller, and splitting the name off is what keeps one owner.
    #[test]
    fn stem_is_a_bare_name() {
        let m = manifest(
            "[package]\nname = \"my_pkg\"\nversion = \"0.1.0\"\n\n\
             [build]\noutput = \"chosen\"\n",
        );
        let stem = artifact_stem(&m, None);
        assert!(
            !stem.contains('/'),
            "stem must not carry a directory: {stem}"
        );
        assert!(
            !stem.contains('.'),
            "stem must not carry an extension: {stem}"
        );
    }
}
