# Known-environmental fuzzer artifacts

Programs the #72 cross-substrate fuzzer staged as `DIVERGENCE REPRODUCER` when the
failure was actually a property of the BUILD, not of the program — a missing
`mlir-build` feature, a feature-subset refusal, a toolchain gap.

They are kept, not deleted, for two reasons:

1. **They are evidence.** Each one records what the fuzzer *did*, which is how the
   misclassification was found in the first place. The artifact for seed
   0xDEADBEEF prog000 changed content between two readings — from an
   `--emit-shared requires 'mlir-build'` message to a backtick-quoted
   `E1001 … requires the \`std-surface\` feature` — and that change is what
   revealed that the capability classifier keyed on single-quoted feature names
   and missed backtick-quoted ones (fixed in c43e748f).
2. **A staged artifact is a claim the tooling made.** Deleting it erases the
   record of a wrong claim while leaving the tooling that made it unexamined.

`is_build_capability_error` in tests/mindfuzz_cross_substrate.rs now prevents new
ones from being staged as divergences at either staging site. These are the
historical ones. They are NOT regression fixtures: nothing runs them, and they do
not describe a compiler defect.
