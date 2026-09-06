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
//! Both spellings historically reached disk. On a host WITH the native backend
//! the orchestrator renames the compile path's file to its own spelling, so
//! `<package.name>` wins; on a host whose entry fell back to the runtime-JIT
//! launcher the build refuses (`build::error::refuse_if_fell_back`) BEFORE that
//! rename, leaving the artifact under the compile path's spelling (`app`). One
//! manifest, two artifact names, decided by a host capability.
//!
//! WHICH refusal that is, is a fact about the host and not a constant: a binary
//! carrying `mlir-build` (every binary that runs the tests below, since cargo
//! builds `CARGO_BIN_EXE_mindc` with this target's own features) refuses with
//! `E5004` when the MLIR tools are absent, and only a binary built WITHOUT the
//! feature ever mints `E5003`. The public CPU path now refuses any runtime-JIT
//! fallback before linking and leaves the directory empty. So the artifact-name
//! claim is stated over what reached disk, never over a cause code; see
//! [`assert_artifact_names`].
//!
//! That divergence is not cosmetic: a downstream harness must hand-type one of
//! the two rules to find the artifact, and picking the wrong one is how a
//! byte-identity check came to panic on exactly the tier it exists for.
//!
//! # What is asserted
//!
//! [`artifact_stem`] is the single resolver; the tests below pin its precedence
//! directly, pin that `mindc build` REPORTS and WRITES that one name on the
//! native path, and pin that a CPU fallback leaves no launcher artifact.
//!
//! Gate: `cargo test --release --features "mlir-build std-surface cross-module-imports" --test mindc_artifact_name`

// ---------------------------------------------------------------------------
// End-to-end: the name `mindc build` reports is the name it wrote
// ---------------------------------------------------------------------------

#[cfg(feature = "mlir-build")]
mod common;

#[cfg(feature = "mlir-build")]
mod e2e {
    use crate::common::{
        gate, reported_artifact, require_mindc, run_build_captured, run_build_captured_env,
    };
    use libmind::diagnostics::capability;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    const HELLO_MIND: &str = "fn main() -> i64 { 42 }\n";

    /// The gate name every skip marker and panic in this module carries. One
    /// spelling, so a `ran=0` in the tier log and a `CRITICAL_<tier>` row can
    /// never name different targets.
    const TARGET: &str = "mindc_artifact_name";

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

    /// THE artifact-name rule, applied to what a build left in `target_dir`.
    ///
    /// `must_exist` decides the STRENGTH of the claim, never its content, and
    /// that split is the whole point: a build that FINISHED, and the
    /// launcher-refusal case driven below, must have left exactly one file and
    /// it must carry `stem`; a refusal this harness did not drive may have left
    /// nothing at all, and then the only claim available is that no OTHER
    /// spelling reached disk.
    ///
    /// Demanding the artifact on every refusal was a red for a non-defect.
    /// Measured on a host with no `~/.mind/lib`: `mindc build` refuses inside
    /// the link with `error[build][E5002]` and `target/debug` is EMPTY, so both
    /// end-to-end tests failed with `left: [] right: ["artifact_name_default"]`
    /// — an assertion naming the artifact-naming defect while reporting an
    /// absent runtime library. `gate::compiled` has already recorded that run as
    /// `ran=0`, which is the honest verdict; the name claim simply has nothing
    /// to bind to.
    ///
    /// A cause code cannot stand in for `must_exist`. `E5003` appears on the
    /// wire BOTH when the launcher was written and when a runtime-library
    /// refusal was out-ranked by it (`FallbackReason::merge`) and nothing was —
    /// and in this module, whose `mindc` always carries `mlir-build`, `E5003` is
    /// never minted at all, so keying on it would leave the branch permanently
    /// unexecuted while still looking like a gate.
    fn assert_artifact_names(target_dir: &Path, stem: &str, must_exist: bool) {
        let left = artifacts_in(target_dir);
        if must_exist {
            assert_eq!(
                left,
                vec![stem.to_string()],
                "exactly one artifact name may reach disk, and it is the one \
                 `project::artifact_stem` yields"
            );
            return;
        }
        assert!(
            left.iter().all(|name| name == stem),
            "the refused build left {left:?} in {}; every artifact a refusal \
             leaves behind must carry the name `project::artifact_stem` yields \
             ({stem})",
            target_dir.display()
        );
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
        let compiled = gate::compiled(TARGET, &out);
        assert_artifact_names(&target_dir, "declared_artifact", compiled);
        if !compiled {
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
        let compiled = gate::compiled(TARGET, &out);

        // Holds on the native path and on the refusal path alike: `app` is not
        // a name any manifest here asked for.
        assert_artifact_names(&target_dir, "artifact_name_default", compiled);

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

    /// A CPU runtime-JIT fallback is refused before linking and leaves no
    /// artifact. The public CPU link accepts genuinely native objects only, so
    /// an installed-runtime stub must not revive the old launcher path.
    #[test]
    fn cpu_fallback_refusal_leaves_no_launcher_artifact() {
        // The runtime-JIT fallback compiles its embedded wrapper with `cc`
        // (`project::compile_embedded_source`), so a host without one cannot
        // reach the launcher-refusal path at all. Routed through the one gate:
        // counted as `ran=0`, and fail-CLOSED under `MIND_BENCH_REQUIRE`, which
        // is set by exactly the tier that installs a toolchain. Should the
        // fallback ever stop shelling out to `cc`, this probe fails LOUD rather
        // than open — the build then refuses uncoded and the assertions below
        // name the refusal they did not expect.
        if Command::new("cc").arg("--version").output().is_err() {
            gate::skipped(
                TARGET,
                "no `cc` on PATH: the runtime-JIT fallback cannot compile its \
                 embedded wrapper, so this host cannot reach the launcher-refusal \
                 path",
            );
            return;
        }

        let dir = crate::common::scratch_dir("mindc_artifact_name_launcher");
        let target_dir = write_project(
            &dir,
            "[package]\nname = \"artifact_name_launcher\"\nversion = \"0.1.0\"\n\n\
             [build]\nentry = \"src/main.mind\"\n",
        );

        // This valid-looking private runtime was enough to drive the historical
        // CPU launcher fallback. CPU linkage must ignore it now.
        let lib_dir = dir.join("stub-runtime");
        fs::create_dir_all(&lib_dir).expect("create stub runtime dir");
        fs::write(
            lib_dir.join("libmind_cpu_linux-x64.so"),
            b"not a shared object\n",
        )
        .expect("write stub runtime");
        let absent_tool = dir.join("no-such-mlir-opt");

        let out = run_build_captured_env(
            &require_mindc(),
            &dir,
            &[],
            &[
                ("MLIR_OPT", absent_tool.as_path()),
                ("MIND_LIB_DIR", lib_dir.as_path()),
            ],
        );

        // NOT routed through `gate::compiled`: this refusal is the SUBJECT of the
        // test, not a gap in the host. Grading it as a capability skip would make
        // the test skip itself, and under `MIND_BENCH_REQUIRE=1` it would hard-fail
        // the tier for reaching exactly the state it asked for.
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "a build whose entry fell back to the runtime JIT must refuse, not \
             report success.\n--- stderr ---\n{stderr}"
        );
        // By CODE, through the compiler's own header parser: the refusal must be
        // the toolchain-absent one this test drove, so a build that refused for
        // some other reason cannot pass for a launcher refusal.
        assert!(
            capability::cause_codes(&stderr).contains(&capability::NATIVE_TOOLCHAIN_ABSENT),
            "expected the toolchain-absent refusal ({}) this test drives; got \
             {:?}.\n--- stderr ---\n{stderr}",
            capability::NATIVE_TOOLCHAIN_ABSENT,
            capability::cause_codes(&stderr)
        );
        assert_artifact_names(&target_dir, "artifact_name_launcher", false);
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
