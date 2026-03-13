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

### [EXPLORE-1] Random strategy + `--seed`
**Expected impact: Medium | Complexity: Low**

Add a `random` selection strategy: at each position, pick a uniformly random valid
candidate from the live pool instead of the first in sorted order. Add a `--seed N`
flag (seeds the RNG) for reproducible runs. Implementation: count live candidates,
pick a random index, walk the rejected bitset to find it. Each run explores a
different path through the search space; useful for sampling the distribution of
valid sequences and discovering paths that greedy misses.

---

### [EXPLORE-2] `min-reject` strategy (most conservative)
**Expected impact: Potentially high | Complexity: Medium**

At each position, among all live candidates, pick the one that would eliminate the
**fewest** remaining candidates if accepted — i.e., the tree whose post-acceptance
sweep marks the smallest number of rejections. This "keep options open" heuristic
may produce significantly longer sequences than greedy-largest by deferring
constraint tightening as long as possible.

Implementation: for each live candidate C, simulate `sweep(C)` and count new
rejections (without committing); pick the C with the minimum count. Cost is
O(live² × embedding) per position in the worst case, but the fingerprint gate
makes most sweeps very cheap. Can be bounded by evaluating only the top-K
candidates by size to limit cost.

---

### [EXPLORE-3] `max-reject` strategy (most aggressive)
**Expected impact: Medium | Complexity: Low (same code as EXPLORE-2)**

Mirror of EXPLORE-2: pick the candidate whose sweep eliminates the **most**
remaining candidates. Burns through the pool aggressively early, potentially
reaching a natural termination faster but with a longer sequence up to that point
(large trees accepted early → fewer conflicts later). Shares almost all
implementation with EXPLORE-2; just flip the min/max comparison.

---

### [EXPLORE-4] Beam search (`--beam-width W`)
**Expected impact: Highest | Complexity: High**

Maintain W partial sequences simultaneously. At each step, expand each beam by
evaluating its top candidates; keep the W longest-running sequences overall.
Properly explores the branching structure of the search space rather than
committing greedily to one path.

Implementation requires W independent `CandidatePool` instances (each with its own
rejected bitset reflecting that beam's acceptance history). Memory cost: W ×
(fingerprint array + rejected bitset) ≈ W × 420 MB at max-nodes=10. With W=4 that
is ~1.7 GB of flat arrays plus W copies of the tree heap.

Suggested approach: restructure `generate_sequence` into a `Beam` struct holding
a `Vec<(Vec<SequenceEntry>, CandidatePool)>`; at each step, each beam picks its
own best candidate, sweeps its own pool, and the beams are ranked by current
length. Beams that terminate early are dropped; new beams can be seeded from the
longest surviving one.

---

### [EXPLORE-5] `partial.json` live output
**Expected impact: High practical utility | Complexity: Very low**

Rewrite `partial.json` (same format as `sequence.json`) after every acceptance,
alongside the existing per-tree SVG writes. If the run is interrupted (Ctrl+C,
OOM, timeout), the file on disk reflects the longest sequence found so far.
The existing `on_found` callback already has everything needed; just add a
`fs::write` call mirroring the overview SVG rewrite.

---

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
