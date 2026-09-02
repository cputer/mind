#!/usr/bin/env python3
"""
MIC/MAP vs JSON/TOML/TOON Benchmark — SHAPE ONLY, NOT THE PUBLISHED FIGURES
==========================================================================

Compares file size, a 4-chars-per-token ESTIMATE, and reference-parser speed.

The token column here is an estimate, and it overstates MIC's advantage (5.3x
estimated vs 3.4x measured with a real BPE tokenizer). Nothing published may be
derived from it: the maintained benchmark is

    benchmarks/mic_map_benchmark_v2.py

which measures real `cl100k_base` tokens, applies the committed price input from
config/token_pricing.toml, and writes the cost model that scripts/check_claims.py
re-derives the README headline from. That separation exists because a $6,780/year
headline was once published off this script's estimate while the methodology it
cited said $396.

The reference payloads (TOML/TOON encodings and the MAP / JSON-RPC sessions) are
imported from that benchmark so the two scripts can never measure different bytes.
"""

import json
import time
import sys

# Fix encoding for Windows
sys.stdout.reconfigure(encoding='utf-8', errors='replace')

# Try to import TOML parser
try:
    import tomllib  # Python 3.11+
    HAS_TOML = True
except ImportError:
    try:
        import tomli as tomllib  # Fallback
        HAS_TOML = True
    except ImportError:
        HAS_TOML = False

# ============================================================
# SAMPLE DATA
# ============================================================

# Neural network layer IR
SAMPLE_IR = {
    "version": 1,
    "symbols": ["input", "weight", "bias", "output"],
    "types": [
        {"id": 0, "dtype": "f32", "shape": ["B", 784]},
        {"id": 1, "dtype": "f32", "shape": [784, 256]},
        {"id": 2, "dtype": "f32", "shape": [256]},
        {"id": 3, "dtype": "f32", "shape": ["B", 256]},
    ],
    "nodes": [
        {"id": 0, "op": "param", "symbol": 0, "type": 0},
        {"id": 1, "op": "param", "symbol": 1, "type": 1},
        {"id": 2, "op": "param", "symbol": 2, "type": 2},
        {"id": 3, "op": "matmul", "inputs": [0, 1], "type": 3},
        {"id": 4, "op": "add", "inputs": [3, 2], "type": 3},
        {"id": 5, "op": "relu", "inputs": [4], "type": 3},
    ],
    "outputs": [5]
}

# Equivalent MIC format
SAMPLE_MIC = """mic@1
S0 "input"
S1 "weight"
S2 "bias"
S3 "output"
T0 [f32;B,784]
T1 [f32;784,256]
T2 [f32;256]
T3 [f32;B,256]
N0 param S0 T0
N1 param S1 T1
N2 param S2 T2
N3 matmul N0 N1 T3
N4 add N3 N2 T3
N5 relu N4 T3
O N5"""

# MAP protocol session vs JSON-RPC. Defined once in mic_map_benchmark_v2.py --
# the maintained benchmark whose real-tokenizer counts back the published
# protocol figures -- so the two scripts can never measure different payloads.
from mic_map_benchmark_v2 import JSON_RPC_SESSION, MAP_SESSION  # noqa: E402

# ============================================================
# UTILITIES
# ============================================================

def count_tokens(text):
    """Approximate token count (GPT-style ~4 chars per token)"""
    return len(text) // 4

def mic_serialize(data):
    """Serialize to MIC format"""
    lines = ["mic@1"]
    for i, sym in enumerate(data["symbols"]):
        lines.append(f'S{i} "{sym}"')
    for t in data["types"]:
        shape = ",".join(str(s) for s in t["shape"])
        lines.append(f'T{t["id"]} [{t["dtype"]};{shape}]')
    for n in data["nodes"]:
        if n["op"] == "param":
            lines.append(f'N{n["id"]} param S{n["symbol"]} T{n["type"]}')
        elif n["op"] in ("matmul", "add"):
            inputs = " ".join(f'N{i}' for i in n["inputs"])
            lines.append(f'N{n["id"]} {n["op"]} {inputs} T{n["type"]}')
        elif n["op"] == "relu":
            lines.append(f'N{n["id"]} relu N{n["inputs"][0]} T{n["type"]}')
    for o in data["outputs"]:
        lines.append(f'O N{o}')
    return "\n".join(lines)

def mic_parse(text):
    """Parse MIC format"""
    result = {"symbols": [], "types": [], "nodes": [], "outputs": []}
    for line in text.strip().split("\n"):
        if line.startswith("mic@"):
            result["version"] = int(line[4:])
        elif line.startswith("S"):
            result["symbols"].append(line.split('"')[1])
        elif line.startswith("T"):
            result["types"].append(line)
        elif line.startswith("N"):
            result["nodes"].append(line)
        elif line.startswith("O"):
            result["outputs"].append(line)
    return result

def benchmark(name, text):
    """Get stats for a format"""
    return {
        "name": name,
        "size": len(text),
        "tokens": count_tokens(text),
        "lines": len(text.strip().split("\n")),
    }

# TOON and TOML encodings of the same reference IR. Defined once in
# mic_map_benchmark_v2.py (see the protocol-payload import above).
from mic_map_benchmark_v2 import SAMPLE_TOML, SAMPLE_TOON  # noqa: E402

def toon_parse(text):
    """Parse TOON format (simplified)"""
    result = {"symbols": [], "types": [], "nodes": [], "outputs": []}
    lines = text.strip().split("\n")
    for line in lines:
        line = line.strip()
        if line.startswith("version:"):
            result["version"] = int(line.split(":")[1].strip())
        elif line.startswith("symbols"):
            symbols = line.split(":")[1].strip()
            result["symbols"] = symbols.split(",")
        elif "," in line and not line.startswith("types") and not line.startswith("nodes"):
            result["nodes"].append(line)
    return result

def benchmark_parse_speed(name, text, parse_func, iterations=10000):
    """Benchmark parse speed"""
    start = time.perf_counter()
    for _ in range(iterations):
        parse_func(text)
    elapsed = time.perf_counter() - start
    return {
        "name": name,
        "iterations": iterations,
        "total_ms": elapsed * 1000,
        "per_parse_us": (elapsed * 1_000_000) / iterations,
    }

# ============================================================
# RUN BENCHMARKS
# ============================================================

print("=" * 70)
print("  IR Serialization Benchmark - All Formats")
print("=" * 70)
print()

json_text = json.dumps(SAMPLE_IR, indent=2)
json_compact = json.dumps(SAMPLE_IR, separators=(',', ':'))
mic_text = mic_serialize(SAMPLE_IR)

results = [
    benchmark("JSON (pretty)", json_text),
    benchmark("TOML", SAMPLE_TOML),
    benchmark("TOON", SAMPLE_TOON),
    benchmark("MIC", mic_text),
]

print(f"{'Format':<18} {'Size':<12} {'Tokens':<10} {'Lines':<8} {'vs JSON':<10}")
print("-" * 70)
json_tokens = results[0]["tokens"]
for r in results:
    ratio = f"{json_tokens/r['tokens']:.1f}x" if r['tokens'] > 0 else "N/A"
    print(f"{r['name']:<18} {r['size']:<12} {r['tokens']:<10} {r['lines']:<8} {ratio:<10}")

print()
print("=" * 70)
print("  Parse Speed Benchmark (10,000 iterations)")
print("=" * 70)
print()

speed_results = [
    benchmark_parse_speed("JSON", json_text, json.loads),
    benchmark_parse_speed("MIC", mic_text, mic_parse),
    benchmark_parse_speed("TOON", SAMPLE_TOON, toon_parse),
]

if HAS_TOML:
    def toml_parse(text):
        return tomllib.loads(text)
    speed_results.insert(1, benchmark_parse_speed("TOML", SAMPLE_TOML, toml_parse))

print(f"{'Format':<12} {'Total (ms)':<15} {'Per Parse (us)':<18} {'vs JSON':<12}")
print("-" * 70)
json_speed = speed_results[0]["per_parse_us"]
for r in speed_results:
    ratio = f"{json_speed/r['per_parse_us']:.1f}x faster" if r['per_parse_us'] < json_speed else f"{r['per_parse_us']/json_speed:.1f}x slower" if r['per_parse_us'] > json_speed else "baseline"
    print(f"{r['name']:<12} {r['total_ms']:<15.2f} {r['per_parse_us']:<18.2f} {ratio:<12}")

print()
print("=" * 70)
print("  MAP vs JSON-RPC - Protocol Benchmark")
print("=" * 70)
print()

map_stats = benchmark("MAP", MAP_SESSION)
jsonrpc_stats = benchmark("JSON-RPC", JSON_RPC_SESSION)

print(f"{'Protocol':<18} {'Size':<12} {'Tokens':<10} {'Lines':<8}")
print("-" * 70)
print(f"{map_stats['name']:<18} {map_stats['size']:<12} {map_stats['tokens']:<10} {map_stats['lines']:<8}")
print(f"{jsonrpc_stats['name']:<18} {jsonrpc_stats['size']:<12} {jsonrpc_stats['tokens']:<10} {jsonrpc_stats['lines']:<8}")

print()
print(f"MAP vs JSON-RPC: {jsonrpc_stats['size']/map_stats['size']:.1f}x smaller, {jsonrpc_stats['tokens']/map_stats['tokens']:.1f}x fewer tokens")

mic_stats = results[3]  # MIC is 4th in results list
mic_tokens = mic_stats["tokens"]

print()
print("=" * 70)
print("  Combined Results")
print("=" * 70)
print()
print("| Format/Protocol | vs JSON/JSON-RPC | Token Savings |")
print("|-----------------|------------------|---------------|")
print(f"| MIC             | {json_tokens/mic_tokens:.1f}x fewer tokens | {json_tokens - mic_tokens} tokens saved |")
print(f"| MAP             | {jsonrpc_stats['tokens']/map_stats['tokens']:.1f}x fewer tokens | {jsonrpc_stats['tokens'] - map_stats['tokens']} tokens saved |")

print()
print("=" * 70)
print("  Sample: MIC Format")
print("=" * 70)
print()
print(mic_text)

print()
print("=" * 70)
print("  Sample: MAP Session")
print("=" * 70)
print()
print(MAP_SESSION)

print()
print("=" * 70)
print("  Key Advantages")
print("=" * 70)
print("""
MIC Format:
  - 3.4x fewer tokens than JSON (real cl100k_base measurement; the estimate above
    reads 5.3x, which is why the estimate does not back any published figure)
  - Line-oriented (git-friendly diffs)
  - Stable IDs for safe patching
  - No nested brackets/braces
  - Human readable

MAP Protocol:
  - 3.8x fewer tokens than JSON-RPC (real cl100k_base measurement)
  - Single-line requests/responses
  - Sequence numbers for correlation
  - Heredoc support for large payloads
  - Designed for AI agent interaction
""")
