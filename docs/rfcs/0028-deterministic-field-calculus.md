# RFC 0028: Deterministic Field Calculus

| Field | Value |
|---|---|
| RFC | 0028 |
| Title | Deterministic field calculus — Q16.16 fields, differential operators, and prove-or-refuse conservation obligations |
| Status | **Draft — Not Scheduled.** No implementation has shipped. Explicitly gated behind the Rust-independence (RI) track and behind RFC 0012 Phase B.2 (§9.1). |
| Authors | STARGA Inc. |
| Created | 2026-08-31 |
| Depends | RFC 0012 (tensor-native surface — `field` extends `Tensor`, not a parallel system), RFC 0024 (loop collapse — the prove-or-refuse + receipt pattern this RFC copies structurally), RFC 0015 (cross-substrate bit-identity — the property a field program may claim), RFC 0016 (evidence-chain emission — carrier for the conservation receipt), RFC 0017 (`mindc verify` — re-derives the receipt) |
| Related | RFC 0006 (mind-blas numeric tiers), RFC 0021 (canonical mic@3 IR), RFC 0027 (governed physical device plane — the *actuation* lane this RFC deliberately does not touch) |

> **Numbering note.** This RFC takes **0028**. `0026` is an unexplained gap in
> `docs/rfcs/` — no reservation record exists anywhere in the tree (unlike `0023`,
> which RFC 0025 documents as reserved-and-cancelled). It is left alone rather than
> reused, because an undocumented gap is more likely a lost reservation than a free
> slot, and the cost of skipping a number is zero.

---

## 1. Summary

This RFC adds **spatial semantics** to MIND's existing shape-typed tensors, so that
the compiler knows a value is a discretized quantity over a domain rather than an
opaque block of numbers — and can therefore *prove* things about it.

Four concepts, no more:

1. `domain` — compile-time spatial metadata (rank, extents, spacing, boundary rule).
2. `field` — `Tensor<T, Shape>` **plus** a domain. Not a parallel type system.
3. Compiler-known differential operators — `grad`, `div`, `laplacian`. Three, not twelve.
4. **Conservation obligations** — `#[conserves(...)]`, which the compiler either proves
   for the given discretization or **refuses to compile**, emitting an `E23xx` diagnostic
   and never silently degrading to a runtime check.

The first normative numeric profile is **Q16.16 only**. This is the load-bearing
restriction and §3 explains why an `f32` field cannot honour MIND's wedge.

The reference workload is not hypothetical: `rfn-mind`'s `laplacian.mind` and
`field_step.mind` already express a Q16.16 5-point stencil with three boundary
conditions in MIND today, by hand. This RFC's success criterion is that those files
become expressible in the field calculus **and lower to byte-identical output**.

### 1.1 What this RFC is not

It is not "Physical AI syntax." An earlier framing of this work proposed roughly
forty keywords spanning physics, meshes, autodiff surface, distribution, device
semantics, precision declarations, optimization and evidence. That framing is
rejected here. An audit of the live tree found that of twelve proposed areas, four
had already shipped, four were owned by other RFCs, and only four were genuinely
absent. This RFC takes only the four absent ones, and takes them at the smallest
size that admits a proof.

---

## 2. Motivation

### 2.1 The compiler currently cannot see space

`rfn-mind/src/laplacian.mind` computes a 5-point stencil. To the compiler that file
is index arithmetic over a flat buffer: a `%` here, a bounds compare there, some
adds. The compiler cannot tell that:

- the loop is a **stencil** (so it has a known halo, and a known parallel structure);
- the `idx` function implements a **boundary condition** (so `Periodic` vs `Reflective`
  is a semantic choice, not an arithmetic accident);
- the result is a **discrete Laplacian** (so `sum(laplacian(F)) == 0` holds exactly
  under periodic BC — a checkable conservation law).

Every one of those facts exists in the programmer's head and in a comment. None is
available to the type-checker, the optimizer, or the evidence chain.

### 2.2 Why a comment is not good enough

The third fact above is the interesting one. Under a periodic boundary condition, a
discrete Laplacian is a difference of a value and its neighbours in which **every
term appears exactly once with coefficient `+1` and once with coefficient `-1`**
across the whole grid. In exact integer arithmetic the global sum is therefore
identically zero — not approximately, exactly. In Q16.16 it is *still* exactly zero,
because Q16.16 addition is integer addition and integer addition is associative.

That is a theorem the compiler could check and the evidence chain could record. Today
it is a sentence in a design doc that nobody re-derives, and a discretization bug that
breaks it produces a plausible-looking simulation that is quietly wrong.

This is precisely the class of failure MIND exists to eliminate.

---

## 3. The Q16.16 restriction (normative, load-bearing)

**The v1 field calculus admits `q16` fields only.** `f32` and `f64` fields are
rejected at the type level with `E2310`.

This is not conservatism about effort. It follows from what the compiler already
knows, and getting it wrong would puncture the wedge:

1. **RFC 0012 already draws this line.** A `#[deterministic]` function operating on
   `f32` without a fixed-point target is specified to emit
   `determinism::f32_non_bit_identical`, signalling that the guarantee is
   within-substrate reproducible but *not* cross-substrate byte-identical
   (RFC 0012 §Open Questions, `docs/rfcs/0012-tensor-native-syntax.md:874-881`).
   Precision matters here: that is an **info** diagnostic and a *proposed resolution*,
   promotable to `warn`/`error` by Mindcraft config — not a hard refusal, and not in
   RFC 0012's normative body. So it is not a precedent for rejecting `f32`; it is
   evidence that the distinction is already recognized. This RFC takes the stronger
   position (`E2310`, a hard error) for the specific reason in point 2: a field
   carrying a *conservation obligation* is making a claim an `f32` field cannot
   discharge, which is a narrower and firmer case than `#[deterministic]` in general.

2. **A conservation proof over floats is not a proof.** `sum(laplacian(F)) == 0` is a
   theorem in `Z/2^64` and in Q16.16 (both are rings under integer addition). Over
   IEEE-754 it is false in general — summation is not associative, so the result
   depends on reduction order, which is exactly the choice MIND pins and other
   compilers leave unspecified. A `#[conserves]` obligation over `f32` could only ever
   be discharged approximately, and an approximate proof recorded in a tamper-evident
   evidence chain is worse than no proof, because it *looks* like one.

3. **The tolerance comparison is itself non-deterministic.** This is the subtle case
   and it is why this RFC has no `tolerance` construct. A contract of the form
   `ensures abs(div(v)) < 1e-5` requires evaluating a float comparison. If the
   residual lands within one ULP of the threshold, x86 and ARM can legitimately
   disagree on PASS/FAIL — and the evidence chain then records a *different verdict
   per machine*. The verification layer would become the one non-reproducible part of
   a system whose entire premise is reproducibility. A float tolerance check does not
   merely fail to help; it actively destroys the property being verified.

Consequently: **v1 obligations are exact integer identities, not tolerances.** There
is no `tolerance` keyword in this RFC. If a future RFC wants approximate conservation,
it must first specify how the comparison itself is made byte-identical (for example
by expressing the bound as an exact Q16.16 quantity and comparing in the ring), and
that is a separate piece of design work with its own proof burden.

An `f32` field lane may be added later **as a non-wedge tier** — clearly marked as
providing shape and operator semantics but *not* cross-substrate identity, in the same
way RFC 0006 has both deterministic and relaxed numeric tiers. It is out of scope here.

---

## 4. Surface syntax

Syntax is presented last-in-importance deliberately: §6's semantics are the
normative content, and punctuation may change before implementation without
re-opening the semantic contract.

### 4.1 `domain`

```mind
domain Grid2D {
    rank: 2,
    extents: [H, W],          // symbolic dims, unified per RFC 0012 Phase A
    spacing: [Q16_ONE, Q16_ONE],
    boundary: Periodic,       // Periodic | Reflective | Zero
}
```

A `domain` is **compile-time metadata**. It emits no code and allocates nothing.
It lowers into existing mic@3 structures as attributes on the field's type; it does
**not** introduce a new IR node. The three boundary rules are exactly the three
`rfn-mind` already implements — not an open-ended set.

### 4.2 `field`

```mind
field F: q16 over Grid2D;            // shorthand
// equivalent to: Tensor<q16, [H, W]> carrying domain Grid2D
```

A `field` **is** a `Tensor` with a domain attached. Every RFC 0012 operator (`@`,
`.+`, `.*`, `.T`, `.sum`, `.mean`) applies unchanged. `field` adds spatial meaning;
it does not add a value representation, a layout, or an allocation strategy.

Field-to-field assignment between different domains is a compile error (`E2311`) —
the same class of protection RFC 0012 gives shape mismatch.

### 4.3 Differential operators

```mind
let L: field q16 over Grid2D = laplacian(F);
let g: field Vec2<q16> over Grid2D = grad(P);
let d: field q16 over Grid2D = div(V);
```

Exactly three operators in v1. `curl`, `dt`, spectral derivatives, arbitrary
coordinate systems, meshes and FEM are **out of scope** — each would need its own
conservation theory, and a field calculus that ships three provable operators is
worth more than one that ships twelve unprovable ones.

Each operator is a **semantic compiler operation**, not a library call. That is the
whole point: because the compiler owns `laplacian`, it may lower the same source to a
CPU stencil, a fused CUDA stencil, or a distributed halo exchange without the program
changing — and it may *prove* things about it, which §5 uses.

---

## 5. Conservation obligations — the prove-or-refuse core

This section is the reason the RFC exists. It copies RFC 0024 `#[collapse]`
**structurally**, because that is the shipped precedent for "the compiler proves it or
refuses to compile," and it is what makes this MIND-native rather than generic
design-by-contract.

### 5.1 The four-part pattern

| Part | `#[collapse]` (shipped, RFC 0024) | `#[conserves]` (this RFC) |
|---|---|---|
| Opt-in attribute | `#[collapse]` on a loop | `#[conserves(...)]` on a field function |
| Prover | `src/opt/collapse.rs` + `scev.rs` | `src/field/conserve.rs` (**does not exist — proposed**) |
| Fail-closed diagnostic | `E22xx`, never silently keeps the loop | `E23xx`, never silently degrades to a runtime check |
| Re-derivable receipt | `evidence_chain.collapse.*`, re-derived by `mindc verify` | `evidence_chain.field.*`, re-derived by `mindc verify` |

The critical inherited property is **fail-closed**. An unprovable `#[collapse]` loop
does not compile. An unprovable `#[conserves]` function must likewise not compile. It
must never fall back to inserting a runtime assertion, because a runtime check is a
different and weaker claim wearing the same syntax.

### 5.2 The v1 obligation set

Exactly two obligations, both exact integer identities in the Q16.16 ring:

**`#[conserves(sum)]`** — the global sum of the output field equals the global sum of
the input field, exactly.

```mind
#[conserves(sum)]
fn diffuse(F: field q16 over Grid2D) -> field q16 over Grid2D {
    return F .+ (A .* laplacian(F));
}
```

Provable because: under `Periodic` boundary, each grid cell contributes `+1` and `-1`
of each neighbour term to the global sum, so `sum(laplacian(F)) == 0` identically in
any ring; therefore `sum(F .+ A .* laplacian(F)) == sum(F)`. The prover checks the
boundary rule is `Periodic`, that the expression is a sum of the input and terms whose
global sum is provably zero, and that all arithmetic is in one ring.

Under `Reflective` or `Zero` boundary the identity does **not** hold — flux crosses
the boundary — and the prover emits `E2302` naming the boundary rule as the reason.
This is a feature: the most common discretization bug in this domain is assuming a
conservation law that the chosen boundary condition does not actually provide.

**`#[conserves(zero_sum)]`** — the output field sums to exactly zero. This is the
direct property of `laplacian` under periodic BC and is the base case the first
obligation builds on.

### 5.3 The `E23xx` diagnostic family

`E22xx` is taken by loop collapse, so field calculus claims `E23xx`:

| Code | Meaning |
|---|---|
| `E2301` | `#[conserves]` expression is not a recognized conservation shape. |
| `E2302` | The obligation does not hold under the domain's boundary rule (names the rule). |
| `E2303` | Mixed-ring arithmetic in a conserved expression — the identity is not ring-closed. |
| `E2304` | Conserved function has a side effect, escaping store, or I/O. |
| `E2305` | An intermediate would need wider-than-64-bit precision to remain exact. |
| `E2310` | Field dtype is not `q16` (see §3 — `f32`/`f64` fields are not admitted in v1). |
| `E2311` | Field domain mismatch across an assignment or operator. |

Following RFC 0024 §4.3, `E2305` is a **permanent** rejection, not a deferral: an
identity that needs arbitrary precision to hold is not an identity in the field's own
ring, and widening it silently would be exactly the kind of invisible semantic change
the wedge forbids.

### 5.4 The receipt

On success the compiler records under `evidence_chain.field.*`: the obligation kind,
the domain's boundary rule, the ring, and the hash of the canonical mic@3 bytes of the
proven function. `mindc verify` **re-derives** this rather than trusting it — same
discipline as RFC 0024's collapse receipt. A receipt that cannot be re-derived is a
verification failure, not a warning.

---

## 6. What this RFC explicitly does NOT own

This section is normative and exists to prevent the scope creep that the earlier
framing of this work exhibited. Each line names the actual owner.

| Not owned here | Owner |
|---|---|
| New IR nodes or a new IR level | RFC 0021 — field metadata rides existing mic@3 structures |
| New evidence primitive or envelope | RFC 0016; `Evidence[K]` is RFC 0004 |
| `precision { ... }` declarations | RFC 0012 `#[target(...)]` + `src/ir/fp_mode.rs`, which *infers* strict/relaxed from the ops rather than trusting an author's assertion — strictly stronger than a declaration |
| Generic `requires`/`ensures` framework | Not proposed. §5 is two named obligations, not a contract language |
| Intent contracts | RFC 0025 |
| Device, actuation, MHS semantics | RFC 0027 — and MHS is an edge codec, never internal representation |
| `jacobian` / `vjp` / `jvp`, `#[autodiff]` surface | The autodiff engine ships (`src/autodiff/`, `diff` type at mic@3 tag `0x08`); its surface annotation is named future work in RFC 0012 §2 and deserves its own small RFC |
| Distributed transport, NCCL, collectives surface | `mind-runtime` (see §9.2 — there is a determinism blocker there) |
| CUDA / kernel surface syntax | `mind-runtime`; lowering is RFC 0014 |
| `optimize` / `solve` / constraint framework | Not proposed by anyone; would need its own RFC |
| Neural-operator architectures | `rfn-mind` incubates; not a language concept |
| Any `f32` cross-substrate identity claim | Nobody — it is not true (§3) |

Note also that `operator` is **not** introduced as new syntax. A field-to-field `fn`
carries the semantics adequately. If evidence later shows a distinct construct buys
something, it can be added; introducing it speculatively would create a second
function-like declaration form with no demonstrated benefit.

---

## 7. Reference workload and success criterion

The gate for this RFC is not a synthetic test. It is:

> `rfn-mind`'s `laplacian.mind` + `field_step.mind`, re-expressed in the field
> calculus, must lower to output **byte-identical** to the current hand-written
> Q16.16 index-arithmetic implementation.

This mirrors RFC 0012 §7.2's IR-text byte-identity gate (`A @ B` ≡
`tensor.matmul(A, B)`) and RFC 0024's requirement that a collapsed loop equal its
un-collapsed form. A field calculus that changes results is not an abstraction, it is
a rewrite.

**Ordering caveat (honest):** `rfn-mind` does not currently compile end-to-end under
public `mindc` (`forensic_audit.md:219`). That blocker (its "E0") must close before
this gate can run. This is a dependency, not an excuse — it is recorded here so the
RFC cannot be declared implementable while its only reference workload does not build.

---

## 8. Why only MIND can do this

`docs/determinism.md:226` states the parallel claim for loop collapse:

> *"No float compiler can do this: it requires a determinism contract on the
> arithmetic itself."*

The same argument carries here, and it is the reason this is a wedge feature rather
than a convenience. A conservation obligation is only checkable if the arithmetic is
associative and the reduction order is pinned. Over IEEE-754 floats with unspecified
reduction order — the default in every mainstream compiler — `sum(laplacian(F))` is
not a well-defined quantity at compile time, so there is nothing to prove. MIND can
prove it precisely because Q16.16 addition is integer addition and MIND pins the
order.

A competitor could add `field` and `laplacian` as syntax tomorrow. They could not add
`#[conserves]`, because they have no ring to prove it in.

---

## 9. Dependencies and blockers

### 9.1 RFC 0012 Phase B.2 (hard prerequisite for the lowering claim)

B.2 is **partial**: `.T`, `.sum` and `.mean` have shipped; `.max` and MLIR-level
byte-identity with `matmul_rmajor_f32_v`/`dot_*_v` remain deferred, both blocked on
**shape-dim threading from the type-checker into `lower_expr`**.

That threading is the same mechanism a field lowering needs — an operator that does
not know its extents cannot emit a stencil. The RFC document does not block on B.2,
but **the field layer may not be declared complete while the compiler loses the
dimensions its own lowering requires.** A beautiful surface over a substrate that
drops shape information is precisely the failure mode MIND exists to avoid.

`.sum` shipping is directly load-bearing here: the v1 obligations in §5.2 are
statements about global sums.

### 9.2 The NCCL determinism question (blocker, not a task)

`mind-runtime` has a real TCP transport but its NCCL path is not complete — the
generic backend returns `Ok(())` with the real `ncclAllReduce` calls left in comments
(`mind-runtime` `src/distributed/backend.rs:231,252`), and the CUDA path logs
`"NCCL not initialized, simulating AllReduce"` (`mind-runtime` `src/backend/cuda/nccl.rs:359`).

**A collective that returns `Ok(())` without communicating is worse than a missing
one, because it is silently green.** That should be fixed on its own merits.

But "finish NCCL" is not merely unfinished plumbing, and must not be scheduled as
such: **NCCL ring-allreduce does not pin reduction order across topologies.** The same
eight GPUs in a different ring produce a different float sum. Completing NCCL as-is
would therefore *puncture* the wedge rather than extend it. Distributed field
execution requires either (a) integer/Q16.16 collectives, where reduction order does
not affect the result, or (b) a pinned deterministic reduction tree. That is an
architectural decision that must be made before any distributed field lowering is
designed, and it is out of scope for this RFC.

### 9.3 Priority

This RFC is **not scheduled**. Rust-independence (RI) is the #1 compiler priority and
this sits behind it. Nothing here should be read as competing for that slot.

---

## 10. Phasing

| Phase | Deliverable | Gate |
|---|---|---|
| A | `domain` + `field` types. Compile-time only: no new MLIR, no new IR node. `E2310`/`E2311` diagnostics. | Existing suite byte-identical; bootstrap oracle unchanged. Mirrors RFC 0012 Phase A exactly. |
| B | `laplacian` as a semantic operator, lowering to the stencil the hand-written form produces. | **Byte-identity** against `rfn-mind`'s `laplacian.mind` (§7). Requires B.2 shape-dim threading (§9.1). |
| C | `grad` + `div`. | Byte-identity against hand-written equivalents. |
| D | `#[conserves(zero_sum)]` + `#[conserves(sum)]`, prover, `E23xx`, `evidence_chain.field.*` receipt. | Prove-or-refuse verified both ways: provable cases compile, and each `E23xx` has a test that *fails to compile*. `mindc verify` re-derives every receipt. |

Phase D is the phase that matters. Phases A–C are the scaffolding that makes it
expressible; D is the part no other compiler can copy.

---

## 11. Open questions

1. **Should `domain` extents be required to be compile-time constant?** Symbolic dims
   (RFC 0012 Phase A) would be more expressive, but a conservation proof may need
   ground extents. Proposed resolution: symbolic dims allowed in the type, ground
   values required at the point a `#[conserves]` obligation is discharged.

2. **Is `Vec2<q16>` needed for `grad`'s result, or should `grad` return a field of
   rank+1?** The latter avoids a new type constructor. Proposed resolution: rank+1,
   pending a look at how `rfn-mind` actually consumes gradient-like quantities.

3. **Does `#[conserves]` compose across function boundaries?** If `f` and `g` each
   conserve sum, `g(f(x))` does too — but proving that requires interprocedural
   reasoning. Proposed resolution: v1 is intraprocedural only; composition is
   explicitly deferred rather than silently assumed.
