# MIC/MAP Patent Reference Benchmark

Executable verification of the quantitative claims in the provisional
patent application "BPE-Optimized Serialization Formats and Secure
Interaction Protocol for Generative AI Compiler Integration" (STARGA
Inc., STARGA Inc.).

## Usage

```bash
pip install tiktoken>=0.7.0
python3 mic_map_benchmark.py
```

## Methodology

The benchmark uses the **original methodology** documented in
`benchmarks/mic_benchmark.py`:

- **Reference IR**: 6-node MLP layer (4 symbols, 4 types, 6 nodes, 1 output)
- **JSON baseline**: `json.dumps(ir, indent=2)` (pretty-printed, verbose keys)
- **Token count**: `len(text) // 4` (standard GPT-style heuristic)

The benchmark ALSO reports real `cl100k_base` tiktoken results as a
secondary view for cross-verification.

## Expected Output

Under the original methodology:

| Format | Bytes | Tokens (~len/4) | vs JSON |
|--------|-------|-----------------|---------|
| JSON (indent=2) | 1117 | 279 | 1.00x |
| MIC v1 | 210 | 52 | 5.37x |
| MIC v2 | 111 | 27 | 10.33x |
| MIC-B binary | 90 | (binary) | 12.41x bytes |

All patent claim thresholds pass under this methodology.

## Real-tokenizer view — the one the public figures use

The `len(text) // 4` column above is retained because the reference thresholds
were stated against it. It is an ESTIMATE and it overstates MIC's advantage. The
public surfaces (README.md, BENCHMARK_RESULTS.md) publish the `cl100k_base`
column instead:

| Format | Bytes | Tokens (`cl100k_base`) | vs JSON |
|--------|-------|------------------------|---------|
| JSON (indent=2) | 1117 | 400 | 1.00x |
| MIC v1 | 210 | 119 | 3.36x |
| MIC v2 | 111 | 71 | 5.63x |

## Cost model

The script also derives the published dollar figure and writes it to the
`cost_model` block of `mic_map_benchmark_results.json`:

- price input: `config/token_pricing.toml` (value, currency, source, review date)
- token counts: the `cl100k_base` column above — the script FAILS CLOSED (exit 1)
  if the tokenizer is unavailable, rather than fall back to the estimate
- `claim_sentence`: the exact sentence every declared surface must carry

`scripts/check_claims.py` recomputes the same arithmetic from those two committed
inputs and fails the build if a surface disagrees, which is what stops a headline
from drifting away from its own methodology (it once read $6,780 against a cited
$396). `python3 tests/check_claims_gate_tests.py` proves that check bites by
mutating each input and requiring the gate to go red.

## Reproducibility

- `sha256` of each payload is captured in the JSON output
- `first_bytes_hex` of each payload is captured for diff-checking
- Output written to `mic_map_benchmark_results.json`
