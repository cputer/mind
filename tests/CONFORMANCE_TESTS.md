# MIND Conformance Test Corpus

This directory contains the **golden test corpus** for MIND Core v1.0 conformance as specified in
[mind-spec/spec/v1.0/conformance.md](https://github.com/star-ga/mind-spec/blob/main/spec/v1.0/conformance.md).

## Test Organization

The corpus is organized into categories corresponding to compiler pipeline stages:

### 1. Lexical Tests (`lexical/`)
- **Purpose**: Validate tokenization, identifiers, literals, keywords, comments
- **Error codes**: E1xxx (lexical errors)
- **Examples**:
  - `valid_identifiers.mind` - Unicode identifiers, underscores, naming conventions
  - `numeric_literals.mind` - Decimal, binary, octal, hex, floating-point formats
  - `invalid_keywords_as_identifiers.mind` - Reserved keyword rejection

### 2. Type Checker Tests (`type_checker/`)
- **Purpose**: Validate type inference, dtype compatibility, trait bounds
- **Error codes**: E2xxx (type errors)
- **Examples**:
  - `basic_type_inference.mind` - Bidirectional inference for primitives
  - `dtype_mismatch.mind` - Incompatible dtype operations

### 3. Shape Inference Tests (`shapes/`)
- **Purpose**: Validate broadcasting, reductions, MatMul/Conv2d shape rules
- **Error codes**: E3xxx (shape errors)
- **Examples**:
  - `broadcast_compatible.mind` - Valid broadcasting scenarios
  - `broadcast_incompatible.mind` - Incompatible shape rejection
  - `matmul_shapes.mind` - MatMul output shape computation

### 4. IR Verification Tests (`ir_verification/`)
- **Purpose**: Validate SSA properties, def-use chains, instruction constraints
- **Error codes**: E4xxx (IR verification errors)
- **Examples**:
  - `ssa_single_assignment.mind` - Valid SSA form
  - `undefined_operand.mind` - Undefined value reference detection

### 5. Autodiff Tests (`autodiff/`)
- **Purpose**: Validate gradient computation for all differentiable operations
- **Error codes**: E5xxx (autodiff errors)
- **Examples**:
  - `simple_gradient.mind` - Scalar gradient d(x²)/dx
  - `matmul_gradient.mind` - MatMul gradient computation per spec rules

### 6. Runtime Execution Tests (`runtime/`)
- **Purpose**: Validate forward execution correctness within numeric tolerances
- **Tolerances**: ≤1e-6 for f32, ≤1e-12 for f64
- **Examples**:
  - `elementwise_add.mind` - Element-wise tensor addition
  - `reduction_sum.mind` - Reduction operations with axis handling

### 7. Backend Selection Tests (`backend/`)
- **Purpose**: Validate CPU/GPU backend availability and error handling
- **Profiles**: CPU baseline (required), GPU profile (optional)
- **Examples**:
  - `cpu_available.mind` - CPU backend always succeeds
  - `gpu_graceful_failure.mind` - GPU unavailable error handling

## Test File Format

Each test file includes:

```mind
// Category: <category>
// Purpose: <what is being tested>
// Expected: <success | error code | specific output>

<test code>
```

For error tests, include the expected error code (E1xxx-E6xxx) in comments.

## Running Conformance Tests

```bash
# Run full conformance suite
cargo test --test conformance

# Run one documented category
cargo test --test conformance -- lexical
cargo test --test conformance -- type_checker
cargo test --test conformance -- shapes
cargo test --test conformance -- ir_verification

# Generate conformance report
cargo test --test conformance -- --nocapture > conformance_report.txt
```

### What those commands assert TODAY (measured, not aspirational)

Each category filter selects a real test that asserts its corpus directory is
present, non-empty, and that every fixture declares an `// Expected:` line. Until
2026-09-05 the filters matched ZERO tests and exited 0, so the documented entry
point to this suite was a guaranteed vacuous green.

They do NOT yet execute the fixtures. The `.mind` files in `lexical/`,
`type_checker/`, `shapes/` and `ir_verification/` are written in a pre-1.0
surface (`tensor<f32[2, 3]>`), and `ir_verification/undefined_operand.mind` says
in its own body that it is a placeholder for an IR-level test — running them
today would assert only that the compiler rejects its own documentation.

The executing profile is separate and small: `libmind::conformance::cpu_cases()`
drives the CPU-baseline profile, and `tests/conformance/cpu_baseline/` +
`tests/conformance/gpu_profile/` are its fixtures. The coverage targets below are
GOALS for v1.0, not a description of the current corpus.

deferred: port each category fixture to the current surface syntax and drive it
through the conformance-cell grid (plan of record Phase 19.3); the per-category
tests then become execution gates instead of corpus-shape gates.

## Conformance Levels

- **CPU Baseline Profile**: All tests except GPU-specific backend tests must pass
- **GPU Profile**: All tests including GPU backend tests must pass

## Coverage Goals

Current coverage targets for v1.0:
- Lexical: 20+ tests covering all token types and error cases
- Type checker: 30+ tests covering inference, generics, traits
- Shapes: 25+ tests covering broadcasting, reductions, matmul, conv2d
- IR verification: 20+ tests covering SSA, verification rules
- Autodiff: 30+ tests covering all Core v1 operations
- Runtime: 40+ tests covering execution correctness
- Backend: 10+ tests covering CPU/GPU selection

**Current status**: 15 tests (initial corpus) → Target: 175+ tests for full coverage

## Contributing Tests

When adding conformance tests:

1. Place in the appropriate category directory
2. Follow naming convention: `<operation>_<scenario>.mind`
3. Include purpose and expected outcome in header comments
4. Reference relevant spec sections (e.g., `autodiff.md:45-67`)
5. Add to test runner in `tests/conformance.rs`

## Test Corpus Versioning

The test corpus is versioned with the specification:
- v1.0.0: Initial conformance corpus for Core v1.0
- v1.0.x: Patch updates (additional tests, clarifications)
- v1.x.0: Minor updates (new operations, non-breaking)
- v2.0.0: Major updates (breaking changes)

## References

- [Conformance Specification](https://github.com/star-ga/mind-spec/blob/main/spec/v1.0/conformance.md)
- [Error Catalog](https://github.com/star-ga/mind-spec/blob/main/spec/v1.0/errors.md)
- [Core IR Specification](https://github.com/star-ga/mind-spec/blob/main/spec/v1.0/ir.md)
