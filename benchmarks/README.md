# MIND Performance Benchmarks

This directory contains benchmark results and performance analysis for the MIND compiler and runtime.

## Latest Results

**Last Updated**: February 17, 2026
**Reference Platform**: Ubuntu 24.04, a commodity x86 CPU, 64GB DDR4, Ampere-class GPU, CUDA 12.8

### Compiler Performance (v0.2.1)

| Metric | Value | Notes |
|--------|-------|-------|
| **scalar_math** | **1.77 µs** | Single arithmetic expression |
| **matmul ops** | **2.6-3.0 µs** | Matrix multiplication programs |
| **medium_mlp** | **6.15 µs** | Multi-layer perceptron (5 ops) |
| **large_network** | **15.49 µs** | Complex network (12 ops) |
| **Throughput** | **~340,000 compiles/sec** | For simple programs |
| **Scaling** | **O(n) with program complexity** | Compile-time scales with number of operations, NOT tensor dimensions |

**Scope**: MIND measures **frontend only** (parse + typecheck + IR lowering). Does not include code generation.

See **[BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)** for detailed methodology and results.
See **[FINAL_PATENT_RESULTS.md](FINAL_PATENT_RESULTS.md)** for patent benchmark documentation.

## External comparison evidence

The committed PyTorch record is [`pytorch_results.json`](pytorch_comparison/pytorch_results.json): PyTorch 2.9.1+cpu with `cuda_available:false`, compared with the MIND CLI subprocess over 10 samples. It records 30.8–52.6× wall-clock ratios, but the harnesses use different invocation scopes, so this is not a tier-matched speedup. No committed GPU result exists.

The historical GPU and JAX cold-start ratios are retained only in old source material and are not publishable benchmark claims without matching artifacts. `mojo/mojo_results.json` contains raw timings, but lacks the complete matched provenance needed for a current ratio.

## Patent Benchmarks

### Quick Start - Run All Benchmarks

```bash
# Run all patent benchmarks
./run_all_benchmarks.sh
```

### Available Benchmarks

| Benchmark | Status | Patent Claims | Description |
|-----------|--------|---------------|-------------|
| **[PyTorch Comparison](pytorch_comparison/)** | ⚠️ CPU artifact | Claims 1-5, 11-15 | Recorded cross-harness comparison |
| **[Determinism Proof](determinism/)** | ✅ Ready | Claims 16-20 | Bit-level reproducibility verification |
| **[Autograd Comparison](autograd_comparison/)** | ✅ Ready | Claims 6-10 | Gradient computation speed & memory |
| **[JAX Comparison](jax_comparison/)** | ⚠️ Raw records | Claims 1-5 | No tier-matched ratio published |
| **[Inference Speed](inference/)** | ✅ Ready | Supporting | Runtime execution comparison |
| **[Mojo Comparison](mojo/)** | ⚠️ Raw artifact | Claims 1-5 | No matched ratio published |

### Individual Benchmark Commands

```bash
# MIND Criterion benchmarks (in-process, most accurate)
cargo bench --bench simple_benchmarks
cargo bench --bench compiler

# PyTorch 2.0 compilation comparison
cd pytorch_comparison && python benchmark_pytorch_compile.py

# Scientific benchmark with video-ready output
python scientific_benchmark.py

# Determinism proof (requires MIND CLI)
cd determinism && python benchmark_determinism.py
```

## MIND Internal Benchmarks

### Quick Start

```bash
# Run all working benchmarks
cargo bench --bench simple_benchmarks
cargo bench --bench compiler

# View results
open target/criterion/report/index.html
```

### Methodology

**Record**: Hardware specs, batch sizes, precision, exact commit hash. Prefer reproducible scripts.

**Baseline Requirements**:
- Clean, idle system
- CPU frequency scaling disabled
- Consistent sample sizes (100 for Criterion, 5-10 for PyTorch cold-start)
- Document outliers and system specs

## Benchmark Results Summary

### Compilation speed evidence

The standalone MIND figures above are T1 frontend measurements. The committed external PyTorch record is CPU-only and cross-harness; it records 30.8–52.6× wall-clock ratios, not a tier-matched speedup. The JAX record and the raw Mojo artifact lack a current tier-matched comparison, so no external ratio is published here.

### Key Findings

1. **Frontend Speed**: MIND frontend compiles in 1.8-15.5 µs (Criterion verified)
2. **Throughput**: ~340,000 frontend compilations per second sustained
3. **Determinism**: Verified bit-identical outputs across 1000+ runs
4. **Autograd**: Compile-time differentiation vs runtime tape-based
5. **Scaling**: Compile time scales with program complexity (O(n) operations), not tensor dimensions
