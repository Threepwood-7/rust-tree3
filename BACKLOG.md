# Backlog

Potential future optimizations and features, roughly ordered by expected impact.

---

## Performance

### [PERF-1] Permanent candidate pruning — ✅ DONE (pass 4)
**Implemented:** `CandidatePool` with `AtomicBool` rejected bitset + post-acceptance
parallel sweep. Scan is now O(N) atomic loads; all embedding work is amortized over
sweeps. Flat arrays (`fingerprints`, `rejected`) pinned in physical RAM via
`mlock`/`VirtualLock`. See CHANGELOG pass 4.

---

### [SCALE-1] Scale to max-nodes=10 with 32 GB RAM
**Expected impact: Massive (explores 7× more trees, finds much longer sequences)**

Memory budget for a 32 GB machine:

| max-nodes | Candidate trees | Heap (trees) | Fingerprints | Rejected bitset | Total |
|-----------|----------------|-------------|--------------|-----------------|-------|
| 9  | 3.5 M  | ~635 MB | ~57 MB  | ~3.4 MB  | ~696 MB  |
| 10 | 24.5 M | ~4.4 GB | ~398 MB | ~23.4 MB | ~4.8 GB  |
| 11 | 171 M  | ~30 GB  | ~2.8 GB | ~163 MB  | ~33 GB   |

Running `--max-nodes 10` is fully within budget (~5 GB). Running `--max-nodes 11`
requires ~33 GB and is borderline. The flat arrays (`fingerprints`, `rejected`) are
already locked in physical RAM via `mlock`/`VirtualLock` (pass 4); just raise the
`--max-nodes` flag. The tree heap (~4.4 GB) is not locked and may page under load —
acceptable since it is accessed sequentially during sweeps.

---

### [PERF-1b] Parallel combo enumeration
**Expected impact: High | Complexity: High**

`gen_combos_cached` (which enumerates child-subtree partitions) is still
sequential due to its recursive `&mut cache` borrow. Once the cache for
sizes 1..n-1 is fully populated, the combo enumeration could be parallelized
per first-child size: each starting size `sz` (1..remaining) is independent.
This would require restructuring the function to take `&cache` (immutable) and
accumulate into a parallel fold. The combo enumeration accounts for the remaining
single-threaded work in the pre-warm phase.

---

### [PERF-2] SIMD label multiset filter
**Expected impact: Small-Medium | Complexity: Medium**

The label multiset check in `embedding.rs` (`label_multiset_fits`) and
`fingerprint.rs` (`compatible`) iterate over a fixed 8-element array.
With `std::simd` (stabilised in Rust nightly), this could be a single 8-lane
u8 comparison and subtraction, reducing the label check to ~2 instructions.
Would benefit most at very high candidate counts (tens of millions).

---

### [PERF-3] Profile-guided accepted-tree ordering
**Expected impact: Medium | Complexity: Low**

In the inner loop `sequence.iter().any(|entry| fingerprint + embeds)`, accepted
trees that reject the most candidates should be checked first. Track per-entry
rejection counts and re-sort the accepted sequence by descending rejection rate
after each position. The hottest rejectors move to the front and short-circuit
the `any()` earlier.

---

### [PERF-4] Compact fixed-size tree encoding
**Expected impact: Medium | Complexity: Medium**

Trees with ≤ 10 nodes and ≤ 3 labels can be encoded in ~32 bytes as a flat
struct (`labels: [u8; 10]`, `n_children: [u8; 10]`, `children: [[u8; 9]; 10]`,
`subtree_sizes: [u8; 10]`). Eliminating `Vec` allocations per node would:
- Improve cache locality during the embedding check
- Make the candidate list a flat `Vec<CompactTree>` with no pointer chasing
- Allow stack-only embedding checks

---

### [PERF-5] Parallel inner sequence check (revisited)
**Expected impact: Low-Medium depending on sequence length | Complexity: Low**

`sequence.par_iter().any()` for the inner loop was tested and showed no gain
when the outer `candidates.par_iter().find_first()` already saturates all threads
(rayon processes nested tasks inline, effectively sequentially). However, for very
long sequences (100+ accepted trees) where the inner loop becomes the bottleneck,
a dedicated thread pool split (e.g., outer uses half the threads, inner uses the
other half) could help. Worth revisiting once sequences exceed ~50 trees.

---

### [PERF-6] Embedding result cache
**Expected impact: Medium | Complexity: Medium**

Cache `embeds(P, C)` results in `HashMap<(canonical_P, canonical_C), bool>`.
Useful if the same (accepted tree, candidate) pair is evaluated multiple times
across positions. Currently each candidate is unique (deduped by canonical form)
and only evaluated once, so the cache hit rate is low for the standard case. Could
become useful with multi-pass strategies or when re-evaluating trees after pruning.

---

## Features

### [FEAT-1] Interactive HTML viewer
Serve the generated SVGs as a single-page HTML file with a grid layout, zoom,
and click-to-highlight embedding relationships between trees.

### [FEAT-2] Graphviz DOT export
Export trees in DOT format for use with `dot`, `neato`, or other Graphviz renderers.
Useful for larger trees where the current SVG layout needs more sophisticated
edge routing.

### [FEAT-3] Tree comparison UI
Given two tree indices, render both side-by-side and visually highlight the
homeomorphic embedding mapping (if one exists) with colored arrows.

### [FEAT-4] Animated sequence growth
Generate an animated SVG or GIF showing trees being added to the sequence
one by one, with embedding rejection visualizations.

---

## Non-starters (investigated, not viable)

### GPU acceleration
The embedding check is a recursive backtracking algorithm with:
- Divergent branching per tree pair (bad for GPU SIMD warps)
- Dynamic stack allocation during recursion (no heap on GPU)
- Irregular memory access via linked tree structures
- Very small trees (≤ 10 nodes) — GPU dispatch overhead would dominate

The data-parallel parts (label filter, size filter) are already nanosecond-fast
on CPU. GPU would add PCIe transfer latency for 3.5M trees with no meaningful
computation to amortize it. Not viable for this problem structure.

### Nested rayon parallelism (inner + outer loop)
Tested: `sequence.par_iter().any()` inside `candidates.par_iter().find_first()`.
No speedup observed. When the outer par_iter saturates all rayon threads, inner
par_iter tasks are processed inline (sequentially) by the current thread.
Rayon's work-stealing cannot create more threads than the pool size.
See [PERF-5] for a viable alternative once sequences are long enough.
