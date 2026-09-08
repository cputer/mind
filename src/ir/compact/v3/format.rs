// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Version contract for the established MIC@3 layouts.

/// Magic header bytes for MIC@3 binary format.
pub const MIC3_MAGIC: [u8; 4] = *b"MIC3";

/// MIC@3 format version byte (the version this build *emits*).
///
/// # Version history
///
/// * `0x01` — original layout. `While.exit_ids` and `If.merges` (the
///   control-flow region-exit / merge metadata) were *not* serialised, so a
///   parsed artifact lost them (`exit_ids: Vec::new()` / `merges: Vec::new()`).
///   Consumer-side `mindc verify` therefore reported false define-before-use
///   errors on control-flow programs whose post-region instructions referenced
///   those exit ids (#24).
/// * `0x02` — appends `While.exit_ids` (a ValueId list) after `init_ids`, and
///   `If.merges` (a `(merge_id, then_val, else_val)` triple list) after
///   `branch_bindings`, so control-flow artifacts are independently
///   SSA-verifiable. This changes the mic@3 bytes for any program containing a
///   `While` with non-empty `exit_ids` or an `If` with non-empty `merges`, and
///   therefore changes `trace_hash` for those programs — the intended effect of
///   making the canonical content *complete* (RFC 0021 step-5).
/// * `0x03` — Step D. Appends the structurally-scoped aggregate `value_types`
///   tables (RFC-canonical array typing): a per-`FnDef` sub-list tail-appended
///   after each function's body, and a module-level table appended after the
///   `repr_c_structs` registry (the last `0x02` section). The version is
///   *content-derived*: `IRModule::has_scoped_value_types` chooses
///   `0x03` iff any scope carries a non-empty table, else the byte-for-byte
///   `0x02` layout ([`MIC3_VERSION_BASE`]). Every table is empty in a
///   pipeline-produced module today (populated only by the later semantic
///   slice), so real programs keep emitting `0x02` unchanged. A `0x03` module
///   with all tables empty is not canonical (it would have derived `0x02`); the
///   parser therefore normalizes an all-empty `0x03` stream back to `0x02` on
///   re-emit (WIRE_NON_CANONICAL_VERSION_NORMALIZES).
///
/// The parser ([`super::parse_mic3`]) accepts `0x01`, `0x02`, and `0x03` in a
/// `std-surface` build: a `0x01` artifact is read with empty `exit_ids` /
/// `merges` (the historical behaviour); a `0x02` artifact reads those but no
/// type tables; a `0x03` artifact reads the type tables too. A bare build
/// explicitly refuses `0x03` before decoding the body, because it cannot
/// preserve the aggregate tables on load/re-emit. The emitter writes
/// [`MIC3_VERSION_BASE`] or [`MIC3_VERSION`] per the content predicate above.
pub const MIC3_VERSION: u8 = 0x03;

/// Draft canonical semantic-metadata layout. Emission is selected only by the
/// checked boundary; legacy content continues to derive v0x02/v0x03.
pub const MIC3_VERSION_V04: u8 = 0x04;

/// The additive-append BASE version — the layout emitted when a module carries
/// no scoped aggregate `value_types` table (the overwhelmingly common case).
/// Its bytes are exactly the historical `0x02` layout, so any real program
/// (which never populates a type table before the semantic slice) is
/// byte-for-byte unchanged from before Step D. See [`MIC3_VERSION`] for the
/// version-derivation contract.
pub const MIC3_VERSION_BASE: u8 = 0x02;

/// Lowest MIC@3 format version this build can *read*. The parser is
/// backward-compatible down to this version; the emitter writes
/// [`MIC3_VERSION_BASE`] or [`MIC3_VERSION`] per the content predicate.
pub const MIC3_MIN_READ_VERSION: u8 = 0x01;
