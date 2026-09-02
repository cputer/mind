//! `[mind]` toolchain-pin enforcement (`Mind.toml`).
//!
//! The `[mind]` table (`mindc-min` / `mindc-max` / `ir-format` /
//! `spec-version`) is the ONLY declared cross-repo compatibility contract in
//! the ecosystem byte boundary. Before this suite existed the manifest loader
//! parsed the table into nothing: an incompatible compiler was
//! indistinguishable from a compatible one at build time, and a typo'd key was
//! indistinguishable from a correct one.
//!
//! These tests pin the fail-CLOSED behaviour where the compiler owns the
//! contract: outside the declared window the manifest load REFUSES (it does not
//! warn), an unknown key INSIDE `[mind]` REFUSES, and an `ir-format` this
//! toolchain cannot actually emit REFUSES. One level up the rule is weaker on
//! purpose and tested as such — an unmodelled TOP-LEVEL table (other tooling
//! reads this file too) is captured and reported, never silently dropped and
//! never a build stop.

use libmind::project::load_manifest;
use std::path::{Path, PathBuf};

/// The version this compiler reports — the same value the enforcement
/// compares against, so the suite cannot drift out of sync with a version bump.
const RUNNING: &str = env!("CARGO_PKG_VERSION");

/// Write a scratch project whose `Mind.toml` is `body`, and return its root.
fn scratch(tag: &str, body: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "mind-pin-test-{}-{}-{}",
        std::process::id(),
        tag,
        line!()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("mkdir scratch project");
    std::fs::write(root.join("Mind.toml"), body).expect("write Mind.toml");
    std::fs::write(
        root.join("src/main.mind"),
        "fn main() -> i64 { return 7; }\n",
    )
    .expect("write entry");
    root
}

const HEADER: &str = "[package]\nname = \"pintest\"\nversion = \"0.1.0\"\n\n[build]\nentry = \"src/main.mind\"\noutput = \"pintest\"\n";

fn err_of(root: &Path) -> String {
    match load_manifest(root) {
        Ok(_) => panic!(
            "load_manifest ACCEPTED a manifest it must refuse: {}",
            root.display()
        ),
        // `{:#}` renders the full anyhow context chain, which is where the
        // manifest path lives.
        Err(e) => format!("{e:#}"),
    }
}

/// The headline defect: a repo pinned to the 0.3.x era must not silently get
/// 0.10.x semantics. The refusal has to name BOTH versions and the file, or an
/// operator cannot act on it.
#[test]
fn out_of_window_max_refuses_and_names_both_versions_and_the_file() {
    let root = scratch(
        "oow-max",
        &format!("{HEADER}\n[mind]\nmindc-min = \"0.2.5\"\nmindc-max = \"0.3.0\"\n"),
    );
    let msg = err_of(&root);
    assert!(msg.contains("0.3.0"), "must name the declared bound: {msg}");
    assert!(
        msg.contains(RUNNING),
        "must name the running compiler: {msg}"
    );
    assert!(msg.contains("Mind.toml"), "must name the file: {msg}");
}

/// A compiler OLDER than the floor is just as incompatible as one past the
/// ceiling — the window is closed on both sides, not just the top.
#[test]
fn below_declared_minimum_refuses() {
    let root = scratch(
        "below-min",
        &format!("{HEADER}\n[mind]\nmindc-min = \"99.0.0\"\nmindc-max = \"100.0.0\"\n"),
    );
    let msg = err_of(&root);
    assert!(
        msg.contains("99.0.0"),
        "must name the declared floor: {msg}"
    );
}

/// `mindc-max` is EXCLUSIVE — the ecosystem pins `max` at the next minor
/// (`0.10.2` / `0.11.0`), so an inclusive reading would admit the whole `0.11`
/// line the repo never declared.
#[test]
fn ceiling_is_exclusive() {
    let root = scratch(
        "ceil-exclusive",
        &format!("{HEADER}\n[mind]\nmindc-min = \"0.0.1\"\nmindc-max = \"{RUNNING}\"\n"),
    );
    let msg = err_of(&root);
    assert!(msg.contains(RUNNING), "must name the boundary: {msg}");
}

/// The live ecosystem window (`mind-nerve`, `rfn-mind`) must keep building.
#[test]
fn in_window_manifest_loads() {
    let root = scratch(
        "in-window",
        &format!(
            "{HEADER}\n[mind]\nmindc-min = \"0.10.0\"\nmindc-max = \"0.11.0\"\nspec-version = \"1.0\"\n"
        ),
    );
    let m = load_manifest(&root).expect("in-window manifest must load");
    assert_eq!(m.package.name, "pintest");
}

/// Back-compat: the overwhelming majority of manifests declare no `[mind]`
/// table at all and must be untouched by this gate.
#[test]
fn absent_mind_section_still_loads() {
    let root = scratch("no-mind", HEADER);
    let m = load_manifest(&root).expect("manifest without [mind] must load");
    assert_eq!(m.package.name, "pintest");
}

/// A typo'd key inside `[mind]` silently disabled the ONLY compatibility
/// contract in the ecosystem. It must be a hard error, not silence.
#[test]
fn typo_key_inside_mind_section_refuses() {
    let root = scratch(
        "typo-key",
        &format!("{HEADER}\n[mind]\nmindc-maxx = \"0.3.0\"\n"),
    );
    let msg = err_of(&root);
    assert!(
        msg.contains("mindc-maxx") || msg.contains("unknown field"),
        "must name the unknown key: {msg}"
    );
}

/// A whole bogus TABLE was accepted silently too — same fail-open class, one
/// level up. It is CAPTURED and reported rather than refused: `Mind.toml` is
/// also read by downstream tooling, so an unmodelled table is not by itself an
/// incompatibility. What must never happen again is silence — the loader has to
/// be able to name it.
#[test]
fn unknown_top_level_table_is_captured_not_silently_dropped() {
    let root = scratch(
        "bogus-table",
        &format!("{HEADER}\n[totally-bogus-table]\nfoo = \"bar\"\n"),
    );
    let m = load_manifest(&root).expect("an unmodelled table is reported, not refused");
    assert!(
        m.unrecognized.contains_key("totally-bogus-table"),
        "the unknown table must be captured, not dropped: {:?}",
        m.unrecognized.keys().collect::<Vec<_>>()
    );
    let note = libmind::project::toolchain_pin::unrecognized_keys_note(
        &m.unrecognized,
        &root.join("Mind.toml"),
    )
    .expect("a captured unknown key must produce an operator-visible note");
    assert!(note.contains("totally-bogus-table"), "{note}");
}

/// A manifest that models every key it uses reports NOTHING — the diagnostic
/// must be silent on the ordinary shape, or operators learn to ignore it.
#[test]
fn fully_modelled_manifest_reports_no_unknown_keys() {
    let root = scratch(
        "clean",
        &format!("{HEADER}\n[mind]\nmindc-min = \"0.0.1\"\n"),
    );
    let m = load_manifest(&root).expect("clean manifest loads");
    assert!(
        m.unrecognized.is_empty(),
        "modelled keys must not be reported as unknown: {:?}",
        m.unrecognized.keys().collect::<Vec<_>>()
    );
}

/// The shape a live ecosystem manifest actually has: an in-window pin plus
/// tables that belong to OTHER tooling (`[determinism]`, `[bit-identity]`) and
/// an `ir-consumer` declaration. None of that is a compatibility problem, so
/// none of it may stop the build — otherwise this gate would fail repos for
/// reasons that have nothing to do with the window it exists to police.
#[test]
fn ecosystem_shaped_manifest_with_third_party_tables_still_builds() {
    let root = scratch(
        "ecosystem",
        &format!(
            "{HEADER}\n[mind]\nmindc-min = \"0.0.1\"\nmindc-max = \"999.0.0\"\n\
             spec-version = \"1.0\"\nir-format = \"mic@3\"\n\
             ir-consumer = \"mind-runtime >= 0.2.4\"\n\
             \n[determinism]\nq16 = true\n\n[bit-identity]\ngate = \"on\"\n"
        ),
    );
    let m = load_manifest(&root).expect("in-window manifest with third-party tables must load");
    assert_eq!(
        m.mind.ir_consumer.as_deref(),
        Some("mind-runtime >= 0.2.4"),
        "a modelled key must be captured, not routed to the unknown bucket"
    );
    let unknown: Vec<&String> = m.unrecognized.keys().collect();
    assert_eq!(
        unknown,
        vec!["bit-identity", "determinism"],
        "third-party tables are reported, and only those"
    );
}

/// `ir-format = "mic@2"` is pinned by a live ecosystem repo, but no `mindc`
/// invocation reaches the mic@2 / MIC-B codec — the toolchain cannot produce
/// it. Accepting the pin would attest a compatibility this compiler does not
/// have.
#[test]
fn ir_format_the_toolchain_cannot_emit_refuses() {
    let root = scratch(
        "ir-mic2",
        &format!("{HEADER}\n[mind]\nir-format = \"mic@2\"\n"),
    );
    let msg = err_of(&root);
    assert!(
        msg.contains("mic@2"),
        "must name the rejected format: {msg}"
    );
    assert!(
        msg.contains("mic@3"),
        "must name what this toolchain CAN emit: {msg}"
    );
}

/// The canonical format is accepted — this gate refuses the unproducible, not
/// the declared.
#[test]
fn ir_format_mic3_accepted() {
    let root = scratch(
        "ir-mic3",
        &format!("{HEADER}\n[mind]\nir-format = \"mic@3\"\n"),
    );
    load_manifest(&root).expect("mic@3 is emittable and must be accepted");
}

/// A malformed version string must fail closed, never parse to a permissive
/// default that admits every compiler.
#[test]
fn unparsable_version_bound_refuses() {
    let root = scratch(
        "bad-semver",
        &format!("{HEADER}\n[mind]\nmindc-max = \"not.a.version\"\n"),
    );
    let msg = err_of(&root);
    assert!(
        msg.contains("not.a.version"),
        "must quote the unparsable bound: {msg}"
    );
}

/// The compiler's OWN manifest must survive the new `deny_unknown_fields` —
/// a gate that breaks the repo it guards is not a gate.
#[test]
fn repo_own_manifest_still_loads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    load_manifest(root).expect("the compiler's own Mind.toml must still load");
}

/// `deny_unknown_fields` has a repo-wide blast radius: it turns any key the
/// loader does not model into a hard refusal, so ONE unmodelled table in ONE
/// shipped example breaks that example's build. Walking every in-tree manifest
/// is the only check that scales with the tree — asserting on the root manifest
/// alone would have passed while an example stayed broken.
#[test]
fn every_in_tree_manifest_still_loads() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = Vec::new();
    let mut queue = vec![repo.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // Skip build output and VCS metadata, not source trees.
                if name == ".git" || name == "target" {
                    continue;
                }
                queue.push(path);
            } else if name == "Mind.toml" {
                load_manifest(&dir).unwrap_or_else(|e| {
                    panic!("in-tree manifest {} no longer loads: {e:#}", path.display())
                });
                checked.push(path);
            }
        }
    }
    // A walk that found nothing would pass vacuously; the root manifest alone
    // guarantees a non-zero floor.
    assert!(
        checked.len() >= 2,
        "expected to find the root manifest and at least one example, found {checked:?}"
    );
}

/// `mindc build` renders only the OUTERMOST error, not the anyhow context
/// chain. A refusal whose actionable detail (which key was rejected) lives
/// deeper in the chain is invisible exactly where an operator reads it, so the
/// top-level rendering must name the offending key itself.
#[test]
fn operator_visible_message_names_the_unknown_key() {
    let root = scratch(
        "outer-msg",
        &format!("{HEADER}\n[mind]\nmindc-maxx = \"0.3.0\"\n"),
    );
    let e = load_manifest(&root).expect_err("a typo'd key must refuse");
    // `{}`, deliberately NOT `{:#}` — this is what the CLI actually prints.
    let outer = format!("{e}");
    assert!(
        outer.contains("mindc-maxx"),
        "the operator-visible message must name the rejected key: {outer}"
    );
    assert!(
        outer.contains("Mind.toml"),
        "the operator-visible message must name the file: {outer}"
    );
}
