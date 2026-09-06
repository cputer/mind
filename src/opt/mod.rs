// Copyright 2025 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the “License”);
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an “AS IS” BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Part of the MIND project (Machine Intelligence Native Design).

pub mod collapse;
pub mod comptime;
pub mod fold;
pub mod ir_canonical;
pub mod memory_layout;
pub mod native_opt;
pub mod regalloc_dtk;
pub mod scev;

/// Canonical Q16.16 cosine fixture shared by the std-surface optimizer tests.
/// This is verbatim from `examples/dottie_collapse.mind`; the collapse anchor
/// keeps the test fixture and shipped program in lockstep.
#[cfg(all(test, feature = "std-surface"))]
pub(crate) const CANON_COS: &str = r#"
fn qmul(a: i64, b: i64) -> i64 {
    let p: i64 = a * b;
    let neg: bool = p < 0;
    let mut m: i64 = p;
    if neg {
        m = 0 - p;
    }
    let mut q: i64 = m >> 16;
    let rem: i64 = m & 65535;
    if rem > 32768 {
        q = q + 1;
    } else {
        if rem == 32768 {
            if (q & 1) == 1 {
                q = q + 1;
            }
        }
    }
    if neg {
        return 0 - q;
    }
    return q;
}

// Q16.16 cosine via a fixed degree-8 Horner Taylor in even powers of x:
//   1 - x^2/2 + x^4/24 - x^6/720 + x^8/40320
// with each factorial coefficient baked as a nearest-Q16.16 literal. Domain
// [-1, 1] needs no range reduction for the cos orbit. Uses `qmul` (RNE) so the
// runtime orbit matches the compile-time fold bit-for-bit.
fn cos_q16(x: i64) -> i64 {
    let x2: i64 = qmul(x, x);
    let mut acc: i64 = 2;
    // 1/40320
    acc = qmul(acc, x2) - 91;
    // -1/720
    acc = qmul(acc, x2) + 2731;
    // 1/24
    acc = qmul(acc, x2) - 32768;
    // -1/2
    acc = qmul(acc, x2) + 65536;
    // 1.0
    return acc;
}
"#;
