//! `[mind]` toolchain-pin enforcement for `Mind.toml`.
//!
//! # Why this module exists
//!
//! The `[mind]` table is the ONLY *declared* cross-repo compatibility contract
//! in the ecosystem byte boundary: a downstream repo states the compiler window
//! it was written against (`mindc-min` / `mindc-max`) and the IR interchange
//! format it couples to (`ir-format`). For a long time `ProjectManifest` had no
//! field for the table, so every one of those declarations was parsed into
//! nothing and discarded. The consequence was fail-OPEN in the worst place: a
//! repo pinned to the `0.3.x` era built clean under `0.10.x` without a word, so
//! an INCOMPATIBLE compiler was indistinguishable from a compatible one at
//! build time.
//!
//! # The contract enforced here
//!
//! * The window is **half-open, `[min, max)`** — `mindc-min` inclusive,
//!   `mindc-max` exclusive. That is what the ecosystem manifests already mean:
//!   they pin `max` at the next minor (`0.10.2` / `0.11.0`), so an inclusive
//!   ceiling would admit the entire `0.11` line the repo never declared.
//! * Either bound may be omitted; an omitted bound is simply unbounded on that
//!   side. A bound that is PRESENT but unparsable is an error — never a
//!   permissive default, which would silently re-open the hole.
//! * `ir-format` must name a format this toolchain can actually emit. Declaring
//!   a format the compiler cannot produce is a compatibility claim the compiler
//!   cannot honour.
//! * The `[mind]` table itself is `deny_unknown_fields`: a typo'd key used to
//!   disable the contract silently, which is the same fail-open shape one level
//!   down. Strictness stops at this table because it is the one the COMPILER
//!   owns; unmodelled top-level tables elsewhere in `Mind.toml` belong to other
//!   tooling and are reported by `unrecognized_keys_note` instead of refused
//!   (a shipped ecosystem manifest carrying `[determinism]` or `[protection]`
//!   is not thereby incompatible with this compiler).
//!
//! Enforcement runs inside `load_manifest`, so every path that loads a manifest
//! to build inherits the refusal rather than each caller re-deriving it.
//!
//! deferred: `mindc check` (`src/check/mod.rs`) and `mindc fmt`
//! (`src/fmt/cli.rs`) load the manifest through `if let Ok(..) = load_manifest`,
//! so a pin violation makes them fall back to default config in silence instead
//! of reporting it. That is NOT a fail-open for the build — `build` propagates
//! the error and refuses — but it is a missing diagnostic on two read-only
//! surfaces. Upgrade path: give those two call sites an explicit `Err` arm that
//! surfaces the refusal as a config-level diagnostic, rather than widening this
//! module.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Result, anyhow};
use serde::Deserialize;

/// The compiler version compared against the declared window. Read from the
/// crate version so a release bump cannot leave the gate measuring a stale
/// number.
pub const RUNNING_TOOLCHAIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// IR interchange formats this toolchain can actually emit today.
///
/// `mic@1` is the textual form written by `--emit-mic`; `mic@3` is the
/// canonical binary artifact written by `--emit-mic3` and the object the
/// byte-identity gates compare. `mic@2` / MIC-B are deliberately ABSENT: the
/// v2 codec exists in-tree but no CLI invocation reaches it, so this compiler
/// cannot produce that format and must not accept a manifest claiming it can.
/// That is a statement about the EMITTER only — the v2 codec remains a supported
/// library parse surface (`compact::v2`), which is why the refusal says so
/// rather than implying the format is gone.
pub const EMITTABLE_IR_FORMATS: &[&str] = &["mic@1", "mic@3"];

/// The `[mind]` table of `Mind.toml` — the declared toolchain contract.
///
/// `deny_unknown_fields` is load-bearing: without it a misspelt `mindc-maxx`
/// deserialises to "no ceiling declared" and the repo believes it is pinned
/// when it is not.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MindSection {
    /// Oldest compiler this project declares itself compatible with
    /// (INCLUSIVE). Absent = unbounded below.
    #[serde(default, rename = "mindc-min")]
    pub mindc_min: Option<String>,
    /// First compiler this project does NOT declare compatibility with
    /// (EXCLUSIVE). Absent = unbounded above.
    #[serde(default, rename = "mindc-max")]
    pub mindc_max: Option<String>,
    /// Language-spec revision the sources target. Recorded, not yet enforced —
    /// there is no spec-revision registry in the compiler to check it against,
    /// and inventing one here would be a gate that asserts nothing.
    /// deferred: enforce once a spec-revision registry exists — upgrade path is
    /// to validate against that registry in `enforce`, alongside `ir-format`.
    #[serde(default, rename = "spec-version")]
    pub spec_version: Option<String>,
    /// IR interchange format the project couples to. Validated against
    /// [`EMITTABLE_IR_FORMATS`].
    #[serde(default, rename = "ir-format")]
    pub ir_format: Option<String>,
    /// Downstream consumer of this project's emitted IR, as a name + range
    /// (e.g. `"mind-runtime >= 0.2.4"`). Recorded, not enforced: the compiler
    /// has no view of which consumer is installed, so a check here could only
    /// assert something it cannot observe. Modelled rather than left unknown
    /// because a shipped ecosystem manifest declares it — under
    /// `deny_unknown_fields` an unmodelled REAL key is indistinguishable from a
    /// typo, and refusing it would be a build stop with no compatibility
    /// meaning.
    /// deferred: enforce once the toolchain can observe the installed consumer
    /// version — upgrade path is to resolve the consumer through the dependency
    /// table and compare here, alongside `ir-format`.
    #[serde(default, rename = "ir-consumer")]
    pub ir_consumer: Option<String>,
}

/// A `major.minor.patch` version, compared componentwise.
///
/// Deliberately total and dependency-free: the only versions compared are the
/// crate's own and the two declared bounds, and an input this cannot parse is
/// reported rather than coerced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

fn parse_version(raw: &str) -> Option<Version> {
    // Tolerate a build/pre-release suffix on the RUNNING version only in the
    // sense that it is split off before numeric parsing; the bounds themselves
    // are expected to be plain triples.
    let core = raw.split(['-', '+']).next().unwrap_or(raw);
    let mut parts = core.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = parts.next().unwrap_or("0").trim().parse().ok()?;
    let patch = parts.next().unwrap_or("0").trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Version(major, minor, patch))
}

/// Refuse when the running compiler falls outside the manifest's declared
/// window, or when the manifest declares an IR format this compiler cannot
/// emit.
///
/// `manifest_path` is threaded through purely so the diagnostic can name the
/// file an operator has to edit — a refusal that does not say WHERE the pin
/// lives just moves the search cost onto the operator.
pub fn enforce(section: &MindSection, running: &str, manifest_path: &Path) -> Result<()> {
    let where_ = manifest_path.display();

    let running_v = parse_version(running).ok_or_else(|| {
        anyhow!(
            "toolchain pin: cannot parse the running compiler version {running:?} \
             declared in {where_}'s window check"
        )
    })?;

    if let Some(min_raw) = &section.mindc_min {
        let min_v = parse_version(min_raw).ok_or_else(|| {
            anyhow!(
                "toolchain pin: `mindc-min = {min_raw:?}` in {where_} is not a \
                 major.minor.patch version"
            )
        })?;
        if running_v < min_v {
            return Err(anyhow!(
                "toolchain pin: this project declares `mindc-min = {min_raw:?}` in \
                 {where_}, but the running compiler is {running}. Build refused: the \
                 project was written for a NEWER toolchain than this one. Upgrade the \
                 compiler, or widen the declared window if {running} is genuinely \
                 supported."
            ));
        }
    }

    if let Some(max_raw) = &section.mindc_max {
        let max_v = parse_version(max_raw).ok_or_else(|| {
            anyhow!(
                "toolchain pin: `mindc-max = {max_raw:?}` in {where_} is not a \
                 major.minor.patch version"
            )
        })?;
        // EXCLUSIVE ceiling — see the module docs.
        if running_v >= max_v {
            return Err(anyhow!(
                "toolchain pin: this project declares `mindc-max = {max_raw:?}` \
                 (exclusive) in {where_}, but the running compiler is {running}. Build \
                 refused: {running} is outside the declared compatibility window, so \
                 its semantics were never validated against these sources. Re-validate \
                 and raise `mindc-max`, or build with a compiler below {max_raw}."
            ));
        }
    }

    if let Some(fmt) = &section.ir_format {
        if !EMITTABLE_IR_FORMATS.contains(&fmt.as_str()) {
            return Err(anyhow!(
                "toolchain pin: this project declares `ir-format = {fmt:?}` in {where_}, \
                 but this compiler can only emit {emittable:?}. Build refused: no \
                 `mindc` invocation produces {fmt:?}, so accepting the pin would attest \
                 an interchange compatibility the toolchain does not have. Migrate the \
                 consumer to `mic@3` (the canonical artifact format), or drop the key \
                 if the project does not consume compiler-emitted IR. (mic@2 / MIC-B \
                 still exist as a library PARSE surface — `compact::v2` — this key is \
                 about what the compiler can EMIT.)",
                emittable = EMITTABLE_IR_FORMATS
            ));
        }
    }

    Ok(())
}

/// Render the operator-facing note for top-level `Mind.toml` keys the compiler
/// does not model, or `None` when there are none.
///
/// Split out as a pure function so the wording is testable without capturing the
/// process's stderr, and so the decision "report, do not refuse" sits next to
/// the pin it is deliberately weaker than.
pub fn unrecognized_keys_note(
    unrecognized: &BTreeMap<String, toml::Value>,
    manifest_path: &Path,
) -> Option<String> {
    if unrecognized.is_empty() {
        return None;
    }
    // BTreeMap iteration is ordered, so the note is byte-stable across runs.
    let keys: Vec<&str> = unrecognized.keys().map(String::as_str).collect();
    Some(format!(
        "mindc: warning: unused manifest key(s) in {}: {} — the compiler does not \
         model these and will not act on them. If one is a typo of a key it does \
         model, the declaration it was meant to make is NOT in effect.",
        manifest_path.display(),
        keys.join(", ")
    ))
}

/// Print [`unrecognized_keys_note`] at most once per manifest path per process.
///
/// One `mindc build` loads the same manifest several times (resolve, build,
/// link). Repeating an identical warning once per load reads like three
/// different problems, which is how a real diagnostic gets trained into
/// background noise — so the message is deduplicated on the path it names.
pub fn report_unrecognized_keys(
    unrecognized: &BTreeMap<String, toml::Value>,
    manifest_path: &Path,
) {
    let Some(note) = unrecognized_keys_note(unrecognized, manifest_path) else {
        return;
    };
    static REPORTED: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    let reported = REPORTED.get_or_init(|| Mutex::new(BTreeSet::new()));
    // A poisoned lock must not swallow the diagnostic: recover the set rather
    // than returning, or a panic elsewhere would silently disable reporting.
    let mut seen = match reported.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if seen.insert(manifest_path.to_path_buf()) {
        eprintln!("{note}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_ordering_is_componentwise_not_lexical() {
        // The lexical trap: "0.10.2" < "0.3.0" as strings, which would invert
        // every window check the ecosystem actually relies on.
        assert!(parse_version("0.10.2").unwrap() > parse_version("0.3.0").unwrap());
        assert!(parse_version("0.10.2").unwrap() < parse_version("0.11.0").unwrap());
    }

    #[test]
    fn short_and_malformed_versions() {
        assert_eq!(parse_version("1"), Some(Version(1, 0, 0)));
        assert_eq!(parse_version("1.2"), Some(Version(1, 2, 0)));
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("not.a.version"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn empty_section_is_unconstrained() {
        let s = MindSection::default();
        assert!(enforce(&s, "0.10.2", Path::new("Mind.toml")).is_ok());
    }

    #[test]
    fn unrecognized_note_is_absent_when_nothing_is_unrecognized() {
        assert!(unrecognized_keys_note(&BTreeMap::new(), Path::new("Mind.toml")).is_none());
    }

    #[test]
    fn unrecognized_note_names_every_key_in_deterministic_order() {
        let mut m = BTreeMap::new();
        m.insert("zeta".to_string(), toml::Value::Boolean(true));
        m.insert("alpha".to_string(), toml::Value::Boolean(true));
        let note = unrecognized_keys_note(&m, Path::new("/p/Mind.toml")).expect("note");
        assert!(note.contains("alpha") && note.contains("zeta"), "{note}");
        assert!(note.contains("/p/Mind.toml"), "{note}");
        // Sorted, not insertion-ordered: the diagnostic must not vary run to run.
        assert!(
            note.find("alpha") < note.find("zeta"),
            "keys must be reported in sorted order: {note}"
        );
    }

    #[test]
    fn the_shipped_compiler_version_parses() {
        // Guards a release bump to a shape this parser rejects, which would
        // turn every pinned manifest into a hard error at once.
        assert!(parse_version(RUNNING_TOOLCHAIN_VERSION).is_some());
    }
}
