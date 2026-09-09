# Pure-MIND native record-array fixtures

These synthetic programs cover the initial fixed-array record-field native
slice. The admitted field form is exactly `[i64; N]`, with `1 <= N <= 4096`.
Construction copies a direct array literal into inline eight-byte cells, and a
whole-field read copies those cells into a fresh array value. Indexed writeback
through `record.field` is deliberately refused.

Same-source record parameters and returns are admitted when the call/return
value has a unique source owner matching the declared array-bearing record.
Unknown, duplicate, or mismatched owners refuse even when their physical sizes
coincide. Standalone array parameters remain outside this slice.

An inline `[i64; N]` initializer must contain expressions whose source shape or
declaration proves an i64 scalar representation. Homogeneous f64 values and
aggregate handles refuse instead of being copied as raw 64-bit cells.
Integer arithmetic, shifts, and bitwise operators remain admitted; comparison
and logical-not results refuse because their source result type is bool.

The gate compares direct and copy-out results with the ordinary MIND evaluator,
executes emitted ELF files, checks scalar native bytes against the frozen
reference, and requires explicit refusal for unsupported array element types,
aliases, nonliteral construction, zero extents, constant OOB, and record-field
writes. Duplicate nominal struct declarations refuse before array-bearing
record transport. Extents above the capability bound or outside checked integer
parsing also refuse. This is a focused single-source native slice; it does not
admit record-field array writeback, standalone array parameters, aliases,
aggregate cells, or multi-module ownership.

The reference checker reports known fixed-array field length and element-type
mismatches before lowering. Its auxiliary facts respect local scopes, constants,
range variables and branch tails; unsupported facts remain deferred. This check
does not broaden the native profile or turn unknown types into native evidence.
