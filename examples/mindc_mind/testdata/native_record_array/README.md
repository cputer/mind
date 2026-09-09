# Pure-MIND native record-array fixtures

These synthetic programs cover the initial fixed-array record-field native
slice. The admitted field forms are exactly `[i64; N]` and `[u8; N]`, with
`1 <= N <= 4096`. Both use an eight-byte logical cell stride. A u8 record cell
stores only its low byte and loads it zero-extended into the ordinary i64-slot
array value; it is not a packed-byte array. Whole-field reads make a fresh
value copy. Exact u8 indexed reads use a bounds-checked direct byte load, while
the established i64 emitted path remains unchanged. Indexed writeback through
`record.field` is deliberately refused.

Same-source record parameters and returns are admitted when the call/return
value has a unique source owner matching the declared array-bearing record.
Unknown, duplicate, or mismatched owners refuse even when their physical sizes
coincide. Standalone array parameters remain outside this slice.

An inline fixed-array initializer must contain expressions whose source shape
or declaration proves the supported integer representation. Homogeneous f64
values and aggregate handles refuse instead of being copied as raw bits.
For u8, admitted integer values normalize modulo 256 on the byte store and load
back in `0..255`. Integer arithmetic, shifts, and bitwise operators remain
admitted; comparison and logical-not results refuse because their source result
type is bool. Untyped lets remain outside this narrow proof surface.

The gate compares direct and copy-out results with the ordinary MIND evaluator,
executes emitted ELF files, checks scalar native bytes against the frozen
reference, and requires explicit refusal for unsupported array element types,
aliases, nonliteral construction, zero extents, constant OOB, and record-field
writes. Both element kinds execute a runtime OOB control. Duplicate nominal
struct declarations refuse before array-bearing record transport. Extents above
the capability bound or outside checked integer parsing also refuse. This is a
focused single-source native slice; it does not admit record-field array
writeback, standalone array parameters, aliases, other narrow widths, aggregate
cells, or multi-module ownership.

The reference checker reports known fixed-array field length and element-type
mismatches before lowering. Its auxiliary facts respect local scopes, constants,
range variables and branch tails; unsupported facts remain deferred. This check
does not broaden the native profile or turn unknown types into native evidence.
