# RFC 0012 Phase B.2 — shape-dim threading: execution scope

| Field | Value |
|---|---|
| Status | **Scoped, not scheduled** — execution packet only |
| Blocks | RFC 0012 B.2 residue (`.reshape`, MLIR byte-identity); RFC 0028 Phase B |
| Priority | Behind RI (Rust-independence remains the #1 compiler priority) |
| Created | 2026-08-31 |

This is a scoping document, not a plan of record. It exists so the work can be
picked up without re-deriving the trace, and so RFC 0028 §9.1's "hard
prerequisite" claim rests on a concrete file:line map rather than an impression.

Every claim below is quoted from the tree at `d3cb4618`.

---

## 1. What is actually deferred

RFC 0012 §7.2 lists five B.2 items. They do **not** share a blocker, and
conflating them overstates the work:

| Item | Status | Blocked on shape threading? |
|---|---|---|
| `.T` transpose | **Shipped** | No |
| `.sum` reduction | **Shipped** | No |
| `.mean` reduction | **Shipped** | No |
| `.max` reduction | Deferred | **No** — see §5 |
| Norm shorthands | Deferred | **No** — see §5 |
| `.reshape` | Deferred | Yes |
| MLIR byte-identity w/ `matmul_rmajor_f32_v` | Deferred | Yes |

Only the last two are genuinely gated. `.max` and the norm shorthands are
ordinary unimplemented features that were listed alongside the blocked pair and
have inherited its "blocked" reputation by adjacency.

---

## 2. Where shapes are known

`ShapeDim` (`src/types/mod.rs:89-92`) is a two-kind enum:

```rust
pub enum ShapeDim {
    Known(usize),
    Sym(&'static str),
}
```

This is load-bearing for RFC 0028 open question 1: symbolic dims already exist
in the type system, and **only `Known` can become a literal call operand.** A
`Sym` dim is not a compile-time extent, so any lowering that needs a ground
value must either require `Known` or refuse.

Shape inference itself is real and working: `reduce_shape`
(`src/type_checker/mod.rs:721`), `engine::infer_output_shape("tensor.matmul", …)`
(`:1355`, `:1405`), and `shape_op_for_binop` (`:1690`).

---

## 3. Where shapes disappear (the exact boundary)

```rust
pub fn check_module_types(module: &Module, src: &str, env: &TypeEnv) -> Vec<Pretty>
```
— `src/type_checker/mod.rs:2700`

**The type checker returns diagnostics and nothing else.** Every shape it infers
is computed, used to decide whether to emit an error, and dropped. All three
call sites consume only diagnostics:

- `src/eval/mod.rs:408`
- `src/pipeline.rs:298`
- `src/check/mod.rs:881`

That is the whole gap. It is not that lowering *loses* the shapes — it is that
the shapes are never handed to it.

### 3.1 The ordering is already favourable

`src/pipeline.rs:298-310` type-checks and then lowers **the same `&module`**:

```rust
let type_diags = type_checker::check_module_types_in_file(&module, …);
if !type_diags.is_empty() { return Err(CompileError::TypeError(type_diags)); }
let mut ir = eval::lower_to_ir(&module);
```

No pass reordering is required. A shape side-table is an additional return
value from a pass that already runs first, over an AST that is still live.

---

## 4. How the shipped B.2 operators cope

This is the question that sizes the work, and the answer is that they sidestep
it entirely.

`.T` parses to `Node::CallTranspose` (`src/parser/mod.rs:4042-4052`) and lowers
(`src/eval/lower.rs:5571-5580`) to:

```rust
Instr::Transpose { dst, src, perm }
```

`Instr::Transpose` (`src/ir/mod.rs:382-386`) carries `{dst, src, perm}` — **no
dimensions**. Likewise `.sum`/`.mean` desugar to the pre-existing
`CallTensorSum`/`CallTensorMean` nodes (`src/parser/mod.rs:3953-3970`).

> **The shipped operators lower to shape-agnostic IR nodes that defer dimension
> resolution downstream.** They did not solve shape threading; they avoided
> needing it.

### 4.1 Stale comments in the test file (a real doc-vs-code gap)

`tests/rfc0012_phase_b_operators.rs:450-475` still documents all three as
deferred — *"Not implemented in Phase B"*, and references a
`Node::TensorTranspose` that does not exist in the AST (the real node is
`CallTranspose`). The tests also assert nothing: `ir_text` (`:94-99`) panics
only on **parse** error, so

```rust
fn phase_b2_transpose_operator() { let _ = ir_text("let t = a.T"); }
```

proves `a.T` parses, not that it lowers correctly. The feature is genuinely
shipped — the parser arms are real — but its tests are smoke tests wearing
deferred-feature comments. Worth fixing independently of this work.

---

## 5. Why `.max` and norms are not blocked

`.sum` and `.mean` reach `reduce_shape` and lower through the existing
reduction path. `.max` is the same shape transformation with a different
reduction kind, and the norm shorthands are compositions of shipped operators.
Neither needs a ground extent at the call site. They are unimplemented, not
blocked — and should not be scheduled as part of this packet.

---

## 6. The byte-identity gap, precisely

The target (`tests/rfc0012_phase_b_operators.rs:484-491`, verbatim):

> This requires threading the type-checker's concrete shape dims (M, K) through
> `lower_expr` to emit `Instr::Call { "__mind_blas_matmul_rmajor_f32_v", [a, b, rows, cols] }`.
> Deferred to Phase B.2 pending type-env integration in lower_expr.

Today `A @ B` lowers (`src/eval/lower.rs:5617-5623`) to a **2-operand** node:

```rust
ir.instrs.push(Instr::MatMul { dst, a, b });
```

The intrinsic is **5-operand** — `("__mind_blas_matmul_rmajor_f32_v", 5, Det::Pure)`
(`src/intrinsics.rs:134`) — and its MLIR emitter is explicitly arity-gated:

```rust
if ikind == Some(IntrinsicKind::MatmulRmajorF32V) && args.len() == 5 {
    self.emit_vec_matmul_rmajor_f32(*dst, args[0], args[1], args[2], args[3], args[4]);
```
— `src/mlir/lowering.rs:2881-2884`

So the two forms cannot emit identical MLIR because they cannot even reach the
same emitter: one is a 2-operand IR node, the other a 5-operand call. The three
missing operands are the output pointer and the two dims — exactly what a shape
table supplies. The identical structure exists for `MatmulRmajorQ16V`
(`src/mlir/lowering.rs:2897`), which is the Q16.16 path RFC 0028 would use.

---

## 7. `.reshape` vs `.sum`

`.sum` collapses extents and needs no ground value at the call site. `.reshape`
**is** its extents: the target shape is the operation. It cannot lower to a
shape-agnostic node, and it must reject a `Sym` target dim (§2) or a
non-volume-preserving one — both of which require `Known` values in
`lower_expr`.

---

## 8. The implementation shape (precedent already in-tree)

`lower_expr` (`src/eval/lower.rs:4981-4997`) already receives **two**
side-tables:

```rust
env: &HashMap<String, ValueId>,                      // bindings
struct_env: &HashMap<String, String>,                // by name
receiver_types: &HashMap<crate::ast::Span, String>,  // by AST span
```

`receiver_types` is the precedent to copy: a module-wide `HashMap<Span, T>`
built once per `lower_to_ir` by a resolver pass
(`crate::eval::struct_resolver::build_field_access_types`, `src/eval/lower.rs:1349`;
builder at `src/eval/struct_resolver.rs:84`, 589 lines). A shape table would be
`HashMap<Span, TensorType>` populated by the type-checker.

### 8.1 The zero-regression discipline is also precedent

`src/eval/lower.rs:1316-1338` gates the resolver walk on a cheap `O(items)`
scan so struct-free modules skip it entirely, and reasons explicitly:

> *"emitted mic@1/mic@3 bytes and cross-substrate identity are byte-for-byte
> unchanged."*

A shape table must do the same — gated on the module actually containing a
tensor-typed construct, empty and never queried otherwise. This is how RFC 0028
§7.1's constitution ("the new prover shouldn't even initialize unless the opt-in
construct is present") is satisfied *mechanically* rather than by intent.

### 8.2 Mechanical cost

`receiver_types` appears in ~15 function signatures in `lower.rs` (`:1882`,
`:2757`, `:2837`, `:2882`, `:3182`, `:3220`, `:3252`, `:3329`, `:3401`, `:3465`,
`:3515`, `:3628`, …). A third side-table replicates that threading. This is
wide, shallow, mechanical churn — low risk per line, but it touches the hottest
file in the compiler, so the bench gate is not a formality.

---

## 9. Affected surface

Changed emission if threading lands **and** call sites are switched:

- `A @ B` → `Instr::Call` 5-operand (`f32` and `q16` variants)
- `.reshape` (new)

Explicitly **unchanged** (must stay byte-identical):

- `.T`, `.sum`, `.mean` — already lower shape-agnostically
- `.+ .- .* ./` → `Instr::BinOp`
- every non-tensor construct — the table is empty for them

Note the tension: switching `A @ B` from `Instr::MatMul` to a 5-operand call
**changes emitted bytes for existing tensor programs**. That is a canary re-bless
requiring an intentional-lowering justification, and it is the single largest
risk in this packet. It may be preferable to keep `Instr::MatMul` as the default
lowering and gate the intrinsic form behind an opt-in, so the byte-identity test
proves *equivalence* without *migrating* the default path.

---

## 10. Gates

Same as any lowering change (`scripts/preflight.sh`, `--full` at `:171`):

```
cargo test --test rfc0012_phase_b_operators          # incl. un-ignoring the 2 deferred tests
scripts/preflight.sh --full
  ├─ keystone byte-identity 7/7   (--test phase_g_keystone_bootstrap -- --test-threads=1)
  ├─ cross-substrate determinism 24/24 (--test cross_substrate_identity)
  ├─ self-host LOOP gate
  └─ bench gate (frozen low-level frontend, --no-default-features)
scripts/check_claims.py
```

The bench gate matters more than usual here: §8.2's threading touches
`lower.rs`, and `src/eval/lower.rs:252` already records type-checking as a
profiled hot path (`infer_expr` 2.1% + `check_module_types` 1.15%). Building
and threading a third side-table is a compile-latency risk, not just a
correctness one.

---

## 11. Open questions

1. **Should `A @ B`'s default lowering migrate?** §9 — migrating changes bytes
   for existing programs; gating keeps them stable but leaves two paths.
   Preference: keep `Instr::MatMul` default, prove equivalence under an opt-in.
2. **Key by `Span` or by a typed expression id?** `receiver_types` uses `Span`
   and works; no expression-id infrastructure exists to reuse.
3. **What does `lower_expr` do on `Sym`?** §2 — a symbolic dim has no ground
   value. Proposed: fall back to the current shape-agnostic lowering rather than
   erroring, so symbolic-shaped code keeps compiling exactly as it does today.
