# MIC3 `0x04` core codec — implementation draft

This unreleased revision preserves canonical semantic metadata supplied to the
reference IR. Its wire grammar and shared vectors remain draft. It does not
establish source-to-native aggregate support, pure-MIND codec parity, or a
released protocol contract.

`emit_mic3_checked` selects `0x04` when the IR carries nonempty canonical
authority. Empty metadata without instruction authority retains legacy output.
The legacy `emit_mic3` entry point does not encode populated semantic metadata;
callers supplying it must use the fallible API. Existing `0x02` and `0x03`
layouts remain unchanged.

## Supported data and instructions

The codec preserves owner-qualified record schemas, declared field order,
function identities and signatures, resolved call targets, and semantic types
keyed by the defining module or function's SSA values. Record references may
form cycles. Fixed and dynamic array descriptors are supported as metadata;
their presence does not make array instructions executable.

| Instruction | Core codec support |
| --- | --- |
| `ConstI64`, `ConstF64`, core `BinOp` variants | Supported |
| `Output`, `Param`, `Return` | Supported |
| `FnDef`, `Call` with complete canonical metadata | Supported |
| Tensor instructions | Refused in this stage |
| Standard-surface instructions and operand variants | Refused in this stage |
| Populated legacy aggregate compatibility sections | Refused in this stage |

Both bare and standard-surface builds use the same core grammar. Required-surface
flags describe content, including operand variants within otherwise core
instructions. Unknown, redundant, or unsupported flags are refused.

## Draft body framing

1. `MIC3` magic, version byte `04`, and required-surface bits.
2. Unique strings sorted by their UTF-8 bytes, with length-prefixed bytes.
3. Sorted schema identity headers, followed by their field lists in the same
   order. Headers precede descriptors so forward record references can resolve.
4. Sorted function declarations with explicit kind and signature descriptors.
5. Module `next_id`, sorted exports, and instructions. Each function has its own
   semantic value table; calls carry a resolved function-table reference.
6. Four always-present compatibility counts, all zero in this core stage,
   followed by module semantic value rows.

Integers use minimal ULEB128 encodings where specified by the codec. Optional
values use explicit `0`/`1` tags. Type tags are explicit constants for scalar,
record reference, fixed array, and dynamic array descriptors. Logical identities
preserve supplied Unicode bytes; owner and name components must be nonempty
and cannot contain `/`, `\`, or `..`. The codec does not infer identities from
host paths or unqualified source names.

The parser returns the exact end of the body. Existing MAP envelope framing
continues from that cursor; a whole-artifact parser rejects trailing content.
Canonical re-emission checks the consumed body byte for byte. Signature
verification remains a separate operation from parsing an envelope.

## Admission limits

Bodies and complete body-plus-MAP artifacts are limited to 10 MiB. Checked
evidence emitters return a structured refusal before appending an oversized
envelope; their infallible wrappers panic on refusal. Descriptor nesting is
limited to 64 and instruction nesting to 256. Fixed extents retain the existing `u32::MAX` bound and semantic
descriptor accounting retains its existing scope. Value IDs must admit safe
exclusive bounds, and module `next_id` must cover module values.

Decoding uses a per-call logical allocation budget of
`min(128 MiB, 1 MiB + 32 * input_bytes)`, with checked charges for entries,
descriptors, strings, and owned copies before construction. This is a portable
admission budget, not an exact resident-memory measurement. Encoding applies
the corresponding budget before returning an artifact, so it cannot publish a
body that its own decoder rejects solely on that budget.

## Verification and remaining work

With `cross-module-imports` enabled, the opt-in Rust library API
`compile_source_to_canonical_ir` binds a source snapshot to a captured project
scope. It preserves resolved function owners and call identities, records scalar
producer types from the existing checker, and verifies returns in their
function scope. Changed snapshots, missing required facts, and unsupported
source forms are refused. Unit functions and functions without an explicit
return annotation remain outside this canonical source slice; ordinary
compilation keeps its existing behavior.
The API returns verified IR before optimization or backend execution.

Controls cover equivalent registries built in different insertion orders,
reused SSA value numbers in separate scopes, owner-qualified calls, recursive
descriptors, malformed encodings, and bounds. Core vectors execute under bare
and standard-surface feature profiles. These checks do not establish identity
across every chip, backend, or compiler implementation.

Full aggregate ownership and source lowering remain unfinished. Standard-surface
opcode transport, pure-MIND
encoder/decoder parity, native consumption, a reviewed shared vector set, and
protocol release remain separate work. Successful scalar transport is not
completion of those dependencies.
