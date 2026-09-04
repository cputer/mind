// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0.
// Part of the MIND project (Machine Intelligence Native Design).

//! Two-sided ratchet over the remaining fail-OPEN skip-and-return sites.
//!
//! # What this gate forbids
//!
//! A wide class of integration-test sites carries the shape
//!
//! ```text
//! if !something_available() { println!("... skipping"); return; }
//! ```
//!
//! without ever consulting `MIND_BENCH_REQUIRE`, so `MIND_BENCH_REQUIRE=1`
//! cannot turn them into a hard failure and the tier can pass vacuously. The
//! fail-CLOSED helper those sites must route through is `common::gate`, whose
//! contract is proven in `tests/fail_closed_capability_skip.rs`. Hand-editing
//! the whole backlog is not the deliverable — the helper is — so the backlog is
//! held under a MECHANICAL ratchet here instead.
//!
//! # Why the ratchet pins a SET and not a count
//!
//! The first version of this ratchet pinned the CARDINALITY (`n == 256`). That
//! does not forbid what it says it forbids: routing one site while adding an
//! unrouted one somewhere else leaves the total at 256 and the gate stays
//! green, so a NEW fail-open site lands undetected — measured on this tree by
//! adding an unrouted `println!("...skipping"); return;` to a file that had
//! none and routing one site in `verify_ssa.rs`; the count-only gate passed.
//! The identities were already computed and thrown away.
//!
//! So the frozen unit is the FILE, with its own site count:
//!
//! * a file OUTSIDE the frozen backlog may hold no site at all — a new
//!   fail-open site in a clean file is red no matter what happens elsewhere;
//! * a frozen file's count may never RISE — a new site next to old ones is red;
//! * a frozen file's count may never silently FALL — draining it demands the
//!   entry be lowered or removed, so the table can never stop meaning what it
//!   says.
//!
//! File PATHS (relative to `tests/`) are frozen, not line numbers, so ordinary
//! edits above a site do not churn the table.
//!
//! deferred: drive `OPEN_SKIP_BACKLOG` to empty by routing each remaining site
//! through `common::gate::{compiled, skipped}` as its file is next touched.
//! Upgrade path and owner: this ratchet reds the moment a file regresses, so no
//! NEW fail-open site can be added while the backlog drains.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Text that proves a skip decision consulted the shared fail-closed helper (or
/// the environment variable it reads). One list, used for every file — the scan
/// scope and the routed-detection live in the same place by construction.
const ROUTED_MARKERS: &[&str] = &[
    "gate::compiled",
    "gate::skipped",
    "gate::classify",
    "enforce_real_backend",
    "MIND_BENCH_REQUIRE",
];

/// The files that still hold unrouted skip-and-return sites, with how many each
/// holds. Measured on this tree; sorted by path, every count non-zero.
/// ENTRIES MAY ONLY EVER SHRINK OR DISAPPEAR — never rise, never be added.
const OPEN_SKIP_BACKLOG: &[(&str, usize)] = &[
    ("alias_miscompile_run.rs", 2),
    ("array_ctor_push_get_run.rs", 2),
    ("array_load_bounds_and_dtype.rs", 4),
    ("array_oob_trap_run.rs", 1),
    ("array_store_run.rs", 3),
    ("array_surface_run.rs", 2),
    ("array_u64_element_shift_run.rs", 2),
    ("bare_variant_ambiguity_run.rs", 2),
    ("bare_variant_ctor_run.rs", 2),
    ("blas_smoke.rs", 1),
    ("blas_vec_q16_smoke.rs", 3),
    ("blas_vec_smoke.rs", 1),
    ("bool_literal_value_run.rs", 2),
    ("bug6_tuple_destructure_u64_run.rs", 2),
    ("bug_f4_closure_shadow_run.rs", 2),
    ("build_run_runnable_blocker_gate.rs", 2),
    ("bytes_buffer_run.rs", 2),
    ("bytes_fixed_into_vec_run.rs", 5),
    ("bytes_zero_run.rs", 2),
    ("chacha20_poly1305_smoke.rs", 1),
    ("char_literal_run.rs", 2),
    ("cli_buffers.rs", 1),
    ("cli_eval.rs", 1),
    ("cli_exec.rs", 1),
    ("cli_tensor.rs", 1),
    ("closure_i64_capture.rs", 4),
    ("collection_ctor_run.rs", 2),
    ("collection_mutation_expr_position_run.rs", 4),
    ("compound_assign.rs", 2),
    ("cond_truthiness.rs", 2),
    ("const_array_run.rs", 2),
    ("const_f64_array_run.rs", 2),
    ("continue_in_match_arm_run.rs", 2),
    ("conv2d_exec.rs", 1),
    ("cross_module_cdylib_compose.rs", 1),
    ("cross_module_enum_run.rs", 2),
    ("cross_module_field_access_run.rs", 2),
    ("cross_substrate_identity.rs", 1),
    ("digit_separator_run.rs", 2),
    ("dot_enum_variant_run.rs", 2),
    ("enum_match_collision_run.rs", 2),
    ("enum_match_run.rs", 2),
    ("enum_struct_variant_run.rs", 2),
    ("extern_narrow_ret_run.rs", 2),
    ("f3_bare_enum_collision_run.rs", 2),
    ("f64_abi_negative_control.rs", 2),
    ("f64_call_arg_run.rs", 2),
    ("f64_literal_envelope.rs", 1),
    ("f64_loop_run.rs", 2),
    ("for_continue_advances_run.rs", 2),
    ("for_each_run.rs", 2),
    ("for_hygiene_run.rs", 2),
    ("g2_differential_mlir.rs", 1),
    ("genref_phase_jb.rs", 4),
    ("int_determinism.rs", 1),
    ("int_suffix_literal.rs", 2),
    ("invariant_block_run.rs", 2),
    ("invariant_check_run.rs", 2),
    ("loop_run.rs", 2),
    ("map_get_inference_run.rs", 2),
    ("map_runtime_run.rs", 2),
    ("map_surface_run.rs", 2),
    ("match_arm_stmt_run.rs", 2),
    ("match_scrutinee_once.rs", 2),
    ("mindc_doc_phase1.rs", 1),
    ("mlir_file_and_lower.rs", 2),
    ("mlir_opt.rs", 1),
    ("module_const_run.rs", 2),
    ("module_decl_run.rs", 2),
    ("module_enum_match_run.rs", 2),
    ("narrow_call_abi.rs", 2),
    ("narrow_local_mask_run.rs", 2),
    ("narrow_locals_leak_run.rs", 2),
    ("narrow_reassign_mask_run.rs", 2),
    ("narrow_reassign_run.rs", 2),
    ("narrow_sig_abi_run.rs", 2),
    ("narrow_signedness_batch2_run.rs", 2),
    ("narrow_signedness_batch_run.rs", 2),
    ("narrow_tuple_pr216_run.rs", 2),
    ("narrow_unsigned_div_zero_run.rs", 1),
    ("nested_block_surface_run.rs", 2),
    ("nested_collection_run.rs", 2),
    ("nested_mut_thread_run.rs", 3),
    ("non_final_catch_all_match_run.rs", 2),
    ("parse_phase10_surface.rs", 2),
    ("pattern_guard_run.rs", 2),
    ("relu_exec.rs", 1),
    ("repl_basic.rs", 1),
    ("result_option_prelude_run.rs", 2),
    ("return_flow_tree_eval_run.rs", 2),
    ("scalar_cast_call_run.rs", 2),
    ("scalar_cast_unsigned_narrow_run.rs", 2),
    ("set_surface_run.rs", 2),
    ("sha256_smoke.rs", 1),
    ("sha512_smoke.rs", 1),
    ("statement_mutation_run.rs", 2),
    ("std_import_standalone_run.rs", 2),
    ("std_llvm_bindings_smoke.rs", 1),
    ("std_mlir_bindings_smoke.rs", 2),
    ("std_surface_arena.rs", 1),
    ("std_surface_async.rs", 1),
    ("std_surface_break_continue.rs", 1),
    ("std_surface_http.rs", 1),
    ("std_surface_i32_intrinsics.rs", 1),
    ("std_surface_io_canon.rs", 3),
    ("std_surface_iouring.rs", 2),
    ("std_surface_json.rs", 2),
    ("std_surface_logical_ops.rs", 1),
    ("std_surface_net_fs_process.rs", 1),
    ("std_surface_promotion_compose.rs", 2),
    ("std_surface_reactor.rs", 1),
    ("std_surface_regex.rs", 1),
    ("std_surface_ring.rs", 1),
    ("std_surface_toml.rs", 1),
    ("std_surface_vec_zeroed.rs", 1),
    ("string_escape_decode_run.rs", 2),
    ("string_from_bytes_run.rs", 2),
    ("string_split_run.rs", 2),
    ("struct_array_field_run.rs", 2),
    ("struct_field_collection_run.rs", 2),
    ("struct_field_in_loop_run.rs", 2),
    ("struct_narrow_field.rs", 2),
    ("substrate_nonentry_import_link.rs", 2),
    ("tensor_param_2d_run.rs", 2),
    ("tensor_param_fail_loud_run.rs", 4),
    ("trait_static_dispatch_run.rs", 3),
    ("try_operator_run.rs", 2),
    ("tuple_destructure_run.rs", 2),
    ("type_struct_run.rs", 2),
    ("typed_literal_match_pattern_run.rs", 2),
    ("u64_cast_signed_compare_run.rs", 2),
    ("value_if_comparison.rs", 2),
    ("value_if_f64_let.rs", 2),
    ("verify_cli.rs", 4),
    ("verify_ssa.rs", 2),
];

/// The frozen backlog as a lookup, with its own well-formedness asserted by
/// `the_frozen_backlog_is_sorted_unique_and_nonzero`.
fn frozen_backlog() -> BTreeMap<&'static str, usize> {
    OPEN_SKIP_BACKLOG.iter().copied().collect()
}

/// The last line index of the print macro starting at `lines[i]`, or `None` if
/// `lines[i]` does not open one outside a comment.
///
/// A single-line match is NOT enough: the announcement this scanner exists to
/// find is routinely wrapped across lines, e.g.
///
/// ```text
/// println!(
///     "target: rebuilt .so is not an ELF - \
///      toolchain unavailable, skipping test"
/// );
/// ```
///
/// where the word "skipping" sits three lines below the macro name. Measured
/// while building this scanner: two such sites read as clean under a
/// line-anchored match. A scanner blind to the wrapped form is the same
/// false-green it is meant to catch, so the span is followed to its `);`.
fn print_macro_span(lines: &[&str], i: usize) -> Option<usize> {
    let t = lines[i].trim_start();
    if t.starts_with("//") {
        return None; // prose quoting the bad shape is not the bad shape
    }
    if !t.contains("println!(") && !t.contains("eprintln!(") {
        return None;
    }
    let last = (i + MAX_MACRO_SPAN).min(lines.len() - 1);
    for (n, line) in lines.iter().enumerate().take(last + 1).skip(i) {
        if line.contains(");") {
            return Some(n);
        }
    }
    Some(last)
}

/// A wrapped print macro is followed at most this many lines to its `);`.
const MAX_MACRO_SPAN: usize = 5;

/// Does the print macro spanning `lines[i..=end]` announce a skip?
fn is_skip_announcement(lines: &[&str], i: usize, end: usize) -> bool {
    lines[i..=end]
        .join("\n")
        .to_ascii_lowercase()
        .contains("skip")
}

/// This scanner's OWN source. It is excluded from the scan because its positive
/// control must quote the very shape it forbids; a scanner that flagged its own
/// specimen jar would be unable to prove it can still see the bad shape.
const SCANNER_SELF: &str = "fail_open_skip_site_ratchet.rs";

/// Every `tests/**/*.rs` source except this scanner, with its path RELATIVE to
/// `tests/` — the identity the frozen backlog is keyed by, so two same-named
/// files in different directories can never be confused for one entry.
fn test_sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).expect("read tests dir") {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut paths = Vec::new();
    walk(&root, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|p| p.file_name().and_then(|s| s.to_str()) != Some(SCANNER_SELF))
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read test source");
            let rel = p
                .strip_prefix(&root)
                .expect("test source under tests/")
                .to_string_lossy()
                .replace('\\', "/");
            (rel, text)
        })
        .collect()
}

/// Sites that announce a skip, return, and never consult the fail-closed helper,
/// as `<path relative to tests/>:<line>`.
fn open_skip_sites() -> Vec<String> {
    let mut open = Vec::new();
    for (rel, text) in test_sources() {
        let lines: Vec<&str> = text.lines().collect();
        for i in 0..lines.len() {
            let Some(end) = print_macro_span(&lines, i) else {
                continue;
            };
            if !is_skip_announcement(&lines, i, end) {
                continue;
            }
            let tail_end = (end + 4).min(lines.len());
            if !lines[i..tail_end].iter().any(|l| l.contains("return")) {
                continue;
            }
            let head = i.saturating_sub(8);
            let window = lines[head..tail_end].join("\n");
            if ROUTED_MARKERS.iter().any(|m| window.contains(m)) {
                continue;
            }
            open.push(format!("{}:{}", rel, i + 1));
        }
    }
    open
}

/// The measured backlog: file -> number of unrouted skip-and-return sites.
fn open_sites_by_file() -> BTreeMap<String, usize> {
    let mut by_file: BTreeMap<String, usize> = BTreeMap::new();
    for site in open_skip_sites() {
        let file = site.rsplit_once(':').expect("site carries a line").0;
        *by_file.entry(file.to_string()).or_insert(0) += 1;
    }
    by_file
}

/// The measured backlog rendered as the frozen table's own syntax, so a failure
/// hands back a paste-ready replacement instead of a puzzle.
fn render_table(by_file: &BTreeMap<String, usize>) -> String {
    by_file
        .iter()
        .map(|(f, n)| format!("    (\"{f}\", {n}),"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_frozen_backlog_is_sorted_unique_and_nonzero() {
    // The table is data the other three tests trust; its shape is asserted here
    // rather than assumed, so a duplicated key cannot silently shadow an entry.
    let mut seen: Vec<&str> = Vec::new();
    for (file, n) in OPEN_SKIP_BACKLOG {
        assert!(*n > 0, "{file} is frozen at 0 sites; delete the entry");
        assert!(!seen.contains(file), "{file} appears twice in the backlog");
        seen.push(file);
    }
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    assert_eq!(seen, sorted, "OPEN_SKIP_BACKLOG must be sorted by path");
    assert_eq!(
        seen.len(),
        frozen_backlog().len(),
        "the backlog lookup lost an entry"
    );
}

#[test]
fn no_file_outside_the_frozen_backlog_holds_a_fail_open_skip_site() {
    // THE DEFECT THIS PINS: a count-only ratchet let a NEW unrouted site land in
    // a clean file as long as one was routed elsewhere. The frozen unit is the
    // file, so a clean file that grows a site is red whatever the total does.
    let frozen = frozen_backlog();
    let measured = open_sites_by_file();
    let strangers: Vec<String> = measured
        .iter()
        .filter(|(f, _)| !frozen.contains_key(f.as_str()))
        .map(|(f, n)| format!("{f} ({n} site(s))"))
        .collect();
    assert!(
        strangers.is_empty(),
        "these files hold unrouted skip-and-return sites and are NOT in the \
         frozen backlog. A skip that never consults MIND_BENCH_REQUIRE cannot \
         be turned into a hard failure, so the tier can pass vacuously. Route \
         the new site through common::gate::{{compiled, skipped}}.\n  {}",
        strangers.join("\n  ")
    );
}

#[test]
fn no_frozen_file_may_gain_a_fail_open_skip_site() {
    let measured = open_sites_by_file();
    let grown: Vec<String> = frozen_backlog()
        .iter()
        .filter_map(|(f, frozen_n)| {
            let now = measured.get(*f).copied().unwrap_or(0);
            (now > *frozen_n).then(|| format!("{f}: {frozen_n} -> {now}"))
        })
        .collect();
    assert!(
        grown.is_empty(),
        "these files gained unrouted skip-and-return sites. Route the new site \
         through common::gate::{{compiled, skipped}}.\n  {}",
        grown.join("\n  ")
    );
}

#[test]
fn a_drained_file_must_be_removed_from_the_frozen_backlog() {
    // The shrink side: when the backlog drains, the table must be lowered, or
    // the freed slack becomes room a future fail-open site could occupy.
    let measured = open_sites_by_file();
    let stale: Vec<String> = frozen_backlog()
        .iter()
        .filter_map(|(f, frozen_n)| {
            let now = measured.get(*f).copied().unwrap_or(0);
            (now < *frozen_n).then(|| format!("{f}: frozen {frozen_n}, now {now}"))
        })
        .collect();
    assert!(
        stale.is_empty(),
        "the backlog shrank; lower or delete these OPEN_SKIP_BACKLOG entries so \
         the ratchet keeps its meaning.\n  {}\n\ncurrent table:\n{}",
        stale.join("\n  "),
        render_table(&measured)
    );
}

/// The exact fail-OPEN string a prior finding named, ASSEMBLED at run time.
///
/// Spelling it as one literal would make this scanner a hit on its own scan and
/// on the shell gate that greps the same text, so the specimen is built from
/// halves. It is still one definition, used by both the assertion and the
/// scanner's positive control.
fn banned_literal() -> String {
    format!("{}{}", "compile failed", "; skipping")
}

#[test]
fn the_converted_compile_sites_are_gone() {
    // The exact fail-OPEN string the finding named. Zero is the whole point.
    let hits: Vec<String> = test_sources()
        .into_iter()
        .filter(|(_, t)| t.contains(&banned_literal()))
        .map(|(p, _)| p)
        .collect();
    assert!(hits.is_empty(), "fail-open compile skips remain: {hits:?}");
}

#[test]
fn the_scanner_itself_can_see_the_bad_shape() {
    // Positive control: a scanner that matches nothing would pass silently.
    fn one(l: &str) -> bool {
        let lines = vec![l];
        print_macro_span(&lines, 0).is_some_and(|e| is_skip_announcement(&lines, 0, e))
    }
    let specimen = format!("    println!(\"arena: mindc {}\");", banned_literal());
    assert!(one(&specimen));
    assert!(one(
        r#"        eprintln!("SKIP: MLIR tools not available");"#
    ));
    assert!(!one(
        r#"    //! println!("... build failed -> skipped"); return;"#
    ));
    assert!(!one(r#"    println!("all good");"#));

    // The wrapped form: the word sits three lines below the macro name.
    let wrapped = vec![
        r#"        println!("#,
        r#"            "target: rebuilt .so is not an ELF - \"#,
        r#"             toolchain unavailable, skipping test""#,
        r#"        );"#,
    ];
    let end = print_macro_span(&wrapped, 0).expect("span must be found");
    assert_eq!(end, 3);
    assert!(is_skip_announcement(&wrapped, 0, end));
}
