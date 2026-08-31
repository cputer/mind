// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License").
// Part of the MIND project (Machine Intelligence Native Design).

//! Distributed primitives for tensor and pipeline parallelism.
//!
//! These are the modules referenced by
//! `bitnet-mind-governance/docs/parallel_pipeline.md` (TP+PP for ternary
//! BitNet). All primitives in this module enforce **deterministic
//! reduction order at compile time** — every collective is keyed by a
//! lexicographic shard ID schedule, never timestamp-arrival order, so
//! cross-shard runs produce bit-identical outputs.
//!
//! Surface in `.mind` source (the names mindc resolves to ops in this
//! module):
//!
//! ```text
//! use mind.distributed.shard
//! use mind.distributed.allreduce
//! use mind.distributed.allgather
//! use mind.distributed.pipeline
//! ```
//!
//! Each submodule below is the compiler-side IR for one primitive.
//!
//! ## Status of the invariants: DEFINED, NOT WIRED
//!
//! `invariants` holds `check_deterministic_all_reduce`,
//! `check_gather_order_lexicographic` and `check_evidence_chain_continuous`. They
//! are correct and unit-tested, and `mindc` does NOT evaluate them: every call
//! site is `#[cfg(test)]` in that file, and nothing in the parser, lowering or
//! emit path ever constructs an `AllReduceOp` / `AllGatherOp` / `PipelineGraph`
//! for them to inspect. `to_mlir` emits whatever `order` it is handed.
//!
//! This doc previously said the invariants "are the compile-time gates `mindc`
//! evaluates before emitting MLIR". That was not true of the shipped compiler and
//! is the reason it is being corrected rather than quietly left: a stated gate
//! that does not run is worse than an absent one, because it is relied on.
//!
//! Wiring them is not a matter of adding a call -- it requires the front end to
//! build these ops in the first place. Until then, treat an arrival-order
//! all-reduce as UNREFUSED by the compiler.
//!
//! deferred: construct the distributed ops during lowering and evaluate these
//! checks there -- upgrade path: a `mind.distributed.*` import becomes a real IR
//! node, and `to_mlir` calls the matching `check_*` before emitting.
//!
//! ## Speed-preservation discipline
//!
//! These primitives are gated by their module-level imports. Source
//! files that don't `use mind.distributed.*` pay zero analysis cost —
//! the existing 1.8–15.5 µs frontend latency stays bit-identical for
//! single-device compiles. See `docs/roadmap.md` Phase 13.6 for the
//! full discipline.

pub mod allgather;
pub mod allreduce;
pub mod invariants;
pub mod pipeline;
pub mod shard;

pub use allgather::{AllGatherOp, GatherOrder};
pub use allreduce::{AllReduceOp, ReductionKind, ReductionOrder};
pub use invariants::{DistributedInvariant, InvariantViolation};
pub use pipeline::{PipelineGraph, PipelineStage, StageBoundary};
pub use shard::{ShardLayout, ShardSpec, ShardingError};

/// Compile-time-fixed cluster topology used by every primitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSize(pub u32);

impl WorldSize {
    pub fn shards(&self) -> u32 {
        self.0
    }
    pub fn requires_collective(&self) -> bool {
        self.0 > 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_size_helpers() {
        assert_eq!(WorldSize(3).shards(), 3);
        assert!(WorldSize(2).requires_collective());
        assert!(!WorldSize(1).requires_collective());
    }
}
