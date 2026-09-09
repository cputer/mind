# Canonical record-trace fixtures

These synthetic programs cover the CANONICAL mic@3 trace for record-field
reads — the `selftest_nb_mic3` path and the note-carrying `selftest_native_elf_u`
executable path that depends on it. Their sibling `native_record_array/` covers
the NATIVE executable path (`selftest_native_elf_h`), which lowers a fixed-array
record field through its own proven inline layout and does not prefold.

`MANIFEST.txt` is the contract, and it is bound to the source: every row records
its fixture's size and sha256 alongside the recorded evidence, so an edited
fixture invalidates its row instead of silently re-baselining it. The manifest
must name exactly the fixtures on disk in both directions.

Roles:

* `trace` — the canonical bytes must equal `mindc --emit-mic3` on the same
  source, the same build must be reproducible within a process, and the emitted
  executable must exit with the recorded status. `scalar_owner.mind` and
  `scalar_owner_named.mind` are the same program with and without the
  `__mind_alloc` / `__mind_store_i64` / `__mind_load_i64` literals in source:
  the prefold rewrites field reads into calls whose callee span is resolvable
  in-source in one case and only in `build_src_intrinsics`' appended copy in the
  other, and both routes must produce the identical module.
* `blocked` — must stay at ZERO bytes on both paths. The reference lowers a
  record with a fixed-array field INLINE, one canonical slot per element, from
  construction onward; this tree's struct-literal desugar still lays every field
  out one slot wide, so folding these reads produced a valid-looking module with
  the wrong bytes (measured: 203 self-host against 279 reference for
  `direct_i64_prefold.mind`). `srt_canonical_layout_modelled` keeps them closed
  on BOTH halves of the gap: `field_owner_layout_modelled` /
  `fixed_field_array_key` close the field READ, and `slit_ctor_layout_key`
  closes the struct-literal CONSTRUCTION desugar. `construct_only.mind` pins the
  second half specifically — it builds a fixed-array record and never reads a
  field, so it is the one fixture the read guard cannot cover. Until it was
  added, that program emitted 129 self-host bytes against the reference's 170:
  every other blocked fixture also performs a read, the read guard fired first,
  and the construction path was never exercised alone. When the inline layout
  lands these fixtures start emitting, the gate goes RED, and they are
  re-derived as `trace` positives — which is the intended signal.
* `refuse` — must stay at zero bytes because the read itself is unsupported or
  invalid: an unadmitted element type, a receiver whose owner is not proven, or
  a statically out-of-range constant index.
* `evaluator` — `reference.mind` restates each executed value as an ordinary
  `#[test]`, so the exit statuses this gate demands come from the reference
  implementation rather than from the artifact under test.

The directory is VCS-ignored on purpose: `mindc check std/ examples/` must not
read a deliberate refusal program as a production-example failure. The gate
therefore re-checks EVERY fixture — all four roles, not just the `trace`
positives — OUTSIDE the ignored tree, paired with a malformed-input control, so
nothing here is assumed to be valid input. That sweep is what makes a zero-byte
row mean something: a `blocked` or `refuse` fixture broken into a syntax error
would emit 0 bytes on both paths and pass forever.
