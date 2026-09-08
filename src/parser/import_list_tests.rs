//! Completeness of the parser-owned import list.
//!
//! BOUNDARY, stated explicitly because it is the reason these live here and not
//! in an integration control: this file tests the PARSER API only. It asserts
//! what `parse_with_imports` records, and nothing about whether a later compiler
//! stage accepts the same program. Several shapes below are recorded by the
//! parser and then REJECTED downstream — by the type checker, by closure
//! admission, or by the frozen native profile. That is the correct division:
//! the collector's job is to know an import was declared, and a stage that
//! refuses the program afterwards does not retroactively make the import
//! undeclared. Testing completeness here is what lets the native controls assert
//! refusal ownership rather than merely a nonzero exit.
//!
//! The list replaces a hand-rolled AST walk that recursed into `Node::Block`
//! only and was measured missing `fn main() { import absent_module; }`.

use super::parse_with_imports;
use crate::parser::parse;

/// Paths recorded for `src`, or a panic naming the parse error.
fn imports_of(src: &str) -> Vec<String> {
    match parse_with_imports(src) {
        Ok((_module, paths)) => paths,
        Err(e) => panic!("fixture must parse, got {e:?}\n--- source ---\n{src}"),
    }
}

fn assert_records(label: &str, src: &str, expected: &str) {
    let got = imports_of(src);
    assert!(
        got.iter().any(|p| p == expected),
        "{label}: expected `{expected}` in the recorded list, got {got:?}"
    );
}

#[test]
fn a_top_level_import_is_recorded() {
    assert_records(
        "top level import",
        "import absent_module;\n\nfn main() -> i64 {\n    return 1;\n}\n",
        "absent_module",
    );
}

#[test]
fn a_top_level_use_is_recorded() {
    assert_records(
        "top level use",
        "use absent_module;\n\nfn main() -> i64 {\n    return 1;\n}\n",
        "absent_module",
    );
}

/// The shape the hand-rolled walk missed entirely.
#[test]
fn an_import_in_a_function_body_is_recorded() {
    assert_records(
        "fn body",
        "fn main() -> i64 {\n    import absent_module;\n    return 1;\n}\n",
        "absent_module",
    );
}

#[test]
fn a_use_in_a_function_body_is_recorded() {
    assert_records(
        "fn body use",
        "fn main() -> i64 {\n    use absent_module;\n    return 1;\n}\n",
        "absent_module",
    );
}

#[test]
fn an_import_in_an_if_branch_is_recorded() {
    assert_records(
        "if branch",
        "fn main() -> i64 {\n    if 1 == 1 {\n        import absent_module;\n    }\n    \
         return 1;\n}\n",
        "absent_module",
    );
}

#[test]
fn an_import_in_an_else_branch_is_recorded() {
    assert_records(
        "else branch",
        "fn main() -> i64 {\n    if 1 == 0 {\n        return 2;\n    } else {\n        \
         import absent_module;\n    }\n    return 1;\n}\n",
        "absent_module",
    );
}

// `while` is a `std-surface` grammar arm; the no-feature parser deliberately
// rejects it, so this container control belongs to the matching feature profile.
#[test]
#[cfg(feature = "std-surface")]
fn an_import_in_a_while_body_is_recorded() {
    assert_records(
        "while body",
        "fn main() -> i64 {\n    while 0 == 1 {\n        import absent_module;\n    }\n    \
         return 1;\n}\n",
        "absent_module",
    );
}

/// UNUSED and UNREACHABLE. The import names nothing the program calls, and the
/// statement sits behind a condition that is never true. The record is written
/// at parse time, so neither fact can hide it. This is the property a
/// reachability- or usage-driven collector would get wrong.
#[test]
fn an_unused_and_unreachable_import_is_still_recorded() {
    assert_records(
        "unused + unreachable",
        "fn main() -> i64 {\n    if 1 == 0 {\n        import absent_module;\n        \
         return 2;\n    }\n    return 1;\n}\n",
        "absent_module",
    );
}

/// A DOTTED path is recorded whole. A last-segment view must never become the
/// authority: `a.helper` and `b.helper` are different modules.
#[test]
fn a_dotted_path_is_recorded_in_full_not_by_last_segment() {
    let got = imports_of("import src.a.helper;\n\nfn main() -> i64 {\n    return 1;\n}\n");
    assert!(
        got.iter().any(|p| p == "src.a.helper"),
        "the full dotted path must be recorded, got {got:?}"
    );
    assert!(
        !got.iter().any(|p| p == "helper"),
        "a last-segment entry must NOT appear: it would collapse a.helper and \
         b.helper into one authority, got {got:?}"
    );
}

/// Two modules sharing a last segment stay distinguishable.
#[test]
fn sibling_paths_sharing_a_last_segment_stay_distinct() {
    let got = imports_of(
        "import src.a.helper;\nimport src.b.helper;\n\nfn main() -> i64 {\n    return 1;\n}\n",
    );
    assert!(
        got.iter().any(|p| p == "src.a.helper") && got.iter().any(|p| p == "src.b.helper"),
        "both full paths must survive, got {got:?}"
    );
}

/// ORDER is declaration order, and dedupe does not disturb it.
#[test]
fn the_list_is_ordered_and_deduped() {
    let got = imports_of(
        "import alpha;\nimport beta;\nimport alpha;\n\nfn main() -> i64 {\n    return 1;\n}\n",
    );
    let locals: Vec<&String> = got.iter().filter(|p| p.as_str() != "std").collect();
    let a = locals.iter().position(|p| p.as_str() == "alpha");
    let b = locals.iter().position(|p| p.as_str() == "beta");
    assert!(
        a.is_some() && b.is_some(),
        "both paths recorded, got {got:?}"
    );
    assert!(a < b, "declaration order preserved, got {got:?}");
    assert_eq!(
        got.iter().filter(|p| p.as_str() == "alpha").count(),
        1,
        "the duplicate is absorbed, got {got:?}"
    );
}

/// `std` IS recorded. Filtering it is the consumer's policy, deliberately not
/// the parser's, so that a consumer which cares about std imports can see them.
#[test]
fn std_is_recorded_and_left_for_the_consumer_to_filter() {
    assert_records(
        "std import",
        "import std.math;\n\nfn main() -> i64 {\n    return 7;\n}\n",
        "std.math",
    );
}

/// ROLLBACK. The parser backtracks in several places (`self.pos = save`) and
/// none of those restores `import_paths`, so a re-parsed region could in
/// principle record twice. It cannot produce a duplicate entry, because the push
/// is guarded by a contains-check — this pins that guard.
///
/// A spurious entry for an import the source never declares would require the
/// keyword sequence to be consumed in a branch that is abandoned and never
/// re-parsed. No accepted shape below produces one. This is a pin on the
/// observed behaviour, not a proof over the whole grammar; over-reporting would
/// be the fail-closed direction, since it can only cause a refusal, never a
/// wrong acceptance.
#[test]
fn backtracking_cannot_produce_a_duplicate_or_phantom_entry() {
    let got = imports_of(
        "import alpha;\n\nfn main() -> i64 {\n    let xs = [1, 2, 3];\n    \
         let n = xs[0];\n    return n;\n}\n",
    );
    assert_eq!(
        got.iter().filter(|p| p.as_str() == "alpha").count(),
        1,
        "exactly one entry survives index-vs-array backtracking, got {got:?}"
    );
    assert!(
        !got.iter().any(|p| p == "xs" || p == "n"),
        "no phantom entry from a backtracked region, got {got:?}"
    );
}

/// A source that does NOT parse yields an error, never an empty list. This is
/// what stops a failed parse being reclassified as "declares no dependency".
#[test]
fn an_unparseable_source_errors_rather_than_reporting_no_imports() {
    assert!(
        parse_with_imports("fn main( -> { this is not mind source\n").is_err(),
        "a failed parse must surface as Err, never as an empty import list"
    );
}

/// `parse` keeps its exact behaviour: an EQUAL AST, compared by `Debug`
/// formatting, and no import side effects. AST equality is not serialized byte
/// identity and is not claimed to be.
#[test]
fn parse_and_parse_with_imports_agree_on_the_module() {
    let src = "import alpha;\n\nfn main() -> i64 {\n    return 1;\n}\n";
    let a = parse(src).expect("parse");
    let (b, paths) = parse_with_imports(src).expect("parse_with_imports");
    assert_eq!(
        format!("{a:?}"),
        format!("{b:?}"),
        "parse() must return an EQUAL module; the import list is additive. This is \
         AST equality, not serialized byte identity."
    );
    assert!(paths.iter().any(|p| p == "alpha"));
}

/// `for` body. Recorded like every other statement position.
#[test]
fn an_import_in_a_for_body_is_recorded() {
    assert_records(
        "for body",
        "fn main() -> i64 {\n    for i in 0..2 {\n        import absent_module;\n    }\n    \
         return 1;\n}\n",
        "absent_module",
    );
}

/// `match` arm body.
#[test]
fn an_import_in_a_match_arm_is_recorded() {
    assert_records(
        "match arm",
        "fn main() -> i64 {\n    let v = 1;\n    match v {\n        _ => {\n            \
         import absent_module;\n            return 1;\n        }\n    }\n}\n",
        "absent_module",
    );
}

/// Closure body, in the explicit-capture form the grammar actually accepts
/// (`|captures; params|`). A `let`-bound closure is a nested statement position
/// that no `Node::Block` walk would have reached.
#[test]
fn an_import_in_a_let_bound_closure_body_is_recorded() {
    assert_records(
        "let-bound closure body",
        "fn main() -> i64 {\n    let f = |; x: i64| {\n        import absent_module;\n        \
         return x;\n    };\n    return f(1);\n}\n",
        "absent_module",
    );
}

/// SHAPES THE GRAMMAR REJECTS, pinned so the boundary is recorded rather than
/// assumed. These are not import-collection gaps: the parser refuses the source
/// outright, so there is no program whose imports could be missed.
///
/// Measured rejections:
/// * `take(import absent_module)` -- "expected ')'". `import` is a STATEMENT,
///   not an expression, so it cannot appear in argument position.
/// * a bare `{ ... }` block inside a function body -- "expected '}'". Not an
///   accepted statement form.
///
/// If either later becomes accepted syntax, this test starts failing, which is
/// the signal to add it to the recorded set above.
#[test]
fn shapes_the_grammar_rejects_are_refused_at_parse_not_silently_collected() {
    let argument_position = "fn take(x: i64) -> i64 {\n    return x;\n}\n\nfn main() -> i64 {\n    \
         return take(import absent_module);\n}\n";
    let bare_block = "fn main() -> i64 {\n    {\n        import absent_module;\n    }\n    \
                      return 1;\n}\n";
    for (label, src) in [
        ("import in argument position", argument_position),
        ("import in a bare block", bare_block),
    ] {
        assert!(
            parse_with_imports(src).is_err(),
            "{label}: the grammar rejected this when measured; if it now parses, \
             add it to the recorded-shape tests above rather than leaving it untested"
        );
    }
}
