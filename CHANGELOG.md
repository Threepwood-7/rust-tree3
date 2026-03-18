# Changelog

All notable changes to this project will be documented in this file.

---

## [Unreleased] — 2026-03-18 (GPU / cudarc)

### Added

**GPU-accelerated post-acceptance sweep** (`--cuda` flag, `--benchmark-sweep` flag)

Full GPU integration via `cudarc` 0.19.3 with two runtime modes:

- **Full-embed mode** (requires nvcc + matching GPU driver): the CUDA kernel in
  `kernels/sweep.cu` performs the complete homeomorphic embedding check on-device.
  Enabled automatically when nvcc is found at build time and the installed GPU
  driver supports the toolkit version.
- **Fp-filter mode** (fallback, always available): the hand-written PTX kernel
  (`kernels/sweep_fp.ptx`, sm_75 / PTX 6.5) performs the fingerprint gate on GPU;
  survivors are checked by CPU rayon.  Works with any CUDA 12.6+ driver.

**`src/gpu_sweep.rs`** (new)
- `GpuFlatTree` (112 bytes, `#[repr(C)]`) and `GpuFlatFP` (20 bytes) — fixed-size
  flat encodings transferred over PCIe; must stay in sync with `kernels/sweep.cu`.
- `GpuSweeper::try_new()`: initialises device memory, tries full-embed PTX, falls
  back to fp-filter PTX.  Stub impl compiled when `cuda` feature is off.
- `GpuSweeper::sweep()`: dispatches fingerprint gate to GPU, runs embedding on CPU
  survivors (fp-filter mode), or runs entire check on device (full-embed mode).
- `GpuSweeper::sync_rejected()`: syncs CPU `AtomicBool` rejection bitset to device.

**`kernels/sweep.cu`** (new)
- Full CUDA C implementation: `fp_compat`, `do_embed`, `can_embed_sub`, `match_ch`.
  Mutual recursion depth ≤ 9; per-thread stack ≤ ~300 bytes (within CUDA default).

**`kernels/sweep_fp.ptx`** (new)
- Hand-written PTX 6.5 (sm_75) for the fingerprint-only kernel.  Embedded directly
  in the binary via `include_str!` — no nvcc required at runtime.

**`build.rs`** (new)
- Auto-detects nvcc in common install paths (Windows + Linux).
- On Windows, finds the newest MSVC `cl.exe` and passes `-ccbin` to nvcc.
- Compiles `sweep.cu` to PTX; patches `.version` header to `8.6` for CUDA 12.6
  driver compatibility.  Non-fatal: falls back to fp-filter mode if nvcc is absent
  or if the driver cannot load the compiled kernel.

**`src/cli.rs`** — added `--cuda` and `--benchmark-sweep` flags to `generate`.

**`src/generator.rs`** — `GenerateOpts`, `SweepTiming`; `run_sweep()` dispatches to
GPU or CPU; `print_benchmark_summary()` reports per-position and totals.

**`DISTRIB.md`** (new) — architecture document covering GPU flat encoding design,
kernel structure, fp-filter vs full-embed analysis, four multi-host distribution
strategies, actual benchmark results, and driver compatibility requirements.

### Changed

- `Cargo.toml`: upgraded `cudarc` to 0.19.3 (`cuda-12060` + `nvrtc` features).
- `src/tests.rs`: all `generate_sequence` call sites updated with `GenerateOpts`.

### Performance (benchmark, GTX 1660, driver 560.94 / CUDA 12.6)

| Mode | CPU rayon | GPU fp-filter | Ratio |
|------|-----------|--------------|-------|
| Per-step sweep (N_active ≈ 2k, N_pool = 502k) | 0.3–0.5 ms | 0.7–1.0 ms | GPU ~3× slower |

GPU fp-filter is slower here because PCIe transfer overhead (502 KB H2D + 502 KB D2H
per step) dominates when the active candidate count is small.  Full-embed mode is
expected to give ~15× speedup on pool-rebuild replay (N=502k), but requires a GPU
driver ≥570.x to load the CUDA 13.2-compiled kernel (current driver: 560.94).

---

## [Unreleased] — 2026-03-13 (pass 4)

### Performance

**Strategy: permanent candidate pruning + physical RAM pinning**

Each accepted tree Tᵢ is used to sweep ALL remaining candidates in parallel
immediately after acceptance. Any candidate C where Tᵢ homeomorphically embeds
into C is permanently banned — marked in a bitset — and skipped at every future
position with a single atomic load. The candidate pool monotonically shrinks
rather than being fully re-evaluated at each position.

#### Changes

**`CandidatePool` struct** (`generator.rs`)
- Replaces the plain `Vec<(String, Tree)>` + ad-hoc `HashSet<canon>` with a
  self-contained pool holding three parallel arrays:
  - `entries: Vec<(String, Tree)>` — strategy-sorted candidates (unchanged)
  - `fingerprints: Vec<TreeFingerprint>` — pre-computed once at pool build;
    eliminates fingerprint recomputation on every sweep and scan
  - `rejected: Vec<AtomicBool>` — permanent rejection bitset; 1 byte per
    candidate, ~24 MB for 24.5 M trees (max-nodes=10)
- `AtomicBool` interior mutability: both the parallel sweep (`par_iter +
  for_each`) and the parallel scan (`par_iter + find_first`) take `&self` —
  no mutex, no pool-level lock.

**Post-acceptance sweep** (`CandidatePool::sweep`)
- After accepting Tᵢ, `par_iter().for_each()` over all non-rejected candidates:
  fingerprint gate (O(1)) then `embeds(Tᵢ, C)` — marks C rejected on match.
- Self-embedding catches the "don't reuse a canonical form" case automatically;
  the separate `used_canons: HashSet` is eliminated.
- Sweep result (newly rejected count) printed to stderr per position.

**O(N)-scan positions** (`CandidatePool::find_first_live`)
- Scan is now a single `par_iter().find_first()` over the `rejected` bitset.
  No fingerprint computation, no embedding check at scan time — all that work
  was done during previous sweeps.

**Physical RAM pinning** (`src/memlock.rs`, new)
- `try_lock_in_ram<T>(slice, label)`: platform-specific best-effort page locking.
  - Windows: `SetProcessWorkingSetSizeEx` to expand the working set, then
    `VirtualLock` to pin pages (requires "Lock pages in memory" privilege for
    large regions; failure is non-fatal).
  - Unix: `mlock` (requires `CAP_IPC_LOCK` or `RLIMIT_MEMLOCK` headroom;
    failure is non-fatal).
- The two flat arrays (`fingerprints`, `rejected`) are locked at pool construction
  time. These are the hottest data touched by every sweep and scan; pinning them
  prevents OS page-out under memory pressure.
- Memory footprint at max-nodes=10: fingerprints ~398 MB + rejected ~23 MB
  = ~421 MB of locked flat arrays; tree objects live in normal heap.

**Pool rebuild on `allowed_size` change**
- When the pool is rebuilt (positions 1..max_nodes as `allowed_size` grows),
  all previously accepted trees are replayed as sweeps to initialize the new
  pool's rejection bitset before the first scan of that size tier.

---

## [Unreleased] — 2026-03-13 (pass 3)

### Performance

**Benchmark: `generate --count 30 --max-nodes 9` (release build)**

| Version | Time | Notes |
|---------|------|-------|
| pass 2 (fingerprint pre-rejection) | 34.5 s | previous baseline |
| pass 3 (parallel tree generation + sort) | **20 s** | ~42% further gain |

**Root cause identified via per-position timing:** 74% of runtime was in the
single-threaded pre-warm phase (tree enumeration for sizes 1–9). The parallel
scan phase was already fast (~9.3 s total); the bottleneck was generating the
3.5 M tree library before any search began.

#### Changes

**Parallel tree construction in `compute_trees_of_size`** (`generator.rs`)
- Once all child-subtree combinations are enumerated (sequential, must read cache
  for sizes < n), the construction of each `(root_label, combo)` tree is
  embarrassingly parallel: each pair is fully independent.
- Changed: collect all `(root_label, &combo)` input pairs cheaply (references, no
  clones), then `par_iter().map()` the expensive `Tree::from_root_and_children` +
  `canonicalize` step across all rayon threads.
- Deduplication now uses `par_sort_unstable` + `dedup_by` instead of a serial
  `HashSet` insertion loop.
- Impact: pre-warm phase drops from ~25 s to ~9 s (~2.8× speedup on that phase,
  consistent with 8-thread machine).

**Parallel candidate list sort** (`generator.rs`)
- `all_trees_up_to_size_largest_first` / `smallest_first` now use
  `par_sort_unstable_by` instead of `sort_by`.
- Sorting 3.5 M entries drops from ~3 s to ~2.1 s.

**Diagnostic print**: added `Parallel workers: N` to startup output so thread
pool utilization is visible to users.

---

## [Unreleased] — 2026-03-13 (pass 2)

### Performance

**Benchmark: `generate --count 30 --max-nodes 9` (release build)**

| Version | Time | Notes |
|---------|------|-------|
| alpha-0.0.1 (baseline) | 148 s | no optimizations |
| rayon + pre-filters only | 128 s | parallel scan + label/size filters |
| + candidate caching + precomputed sizes | **38 s** | all optimizations |
| **Total speedup** | **~3.9×** | |

#### Changes

**Candidate list caching** (`generator.rs`)
- The sorted candidate list (up to 3.5M trees for `--max-nodes 9`) was being rebuilt and re-sorted at every position in the sequence loop.
- Once `position >= max_nodes`, `allowed_size` stops growing and the candidate list is identical for all remaining positions.
- Fix: cache the sorted list and reuse it when `allowed_size` has not changed.
- Impact: eliminates up to `count - max_nodes` redundant O(n log n) sorts over millions of trees.

**Precomputed subtree sizes** (`tree.rs`, `embedding.rs`)
- The `subtree_size(node)` method did an O(n) DFS every time it was called.
- It was called repeatedly inside `embeds_with_sizes`, `match_children`, and `can_embed_in_subtree` — the innermost hot path of the embedding check.
- Fix: added `Tree::all_subtree_sizes() -> Vec<usize>` which computes all subtree sizes in a single O(n) post-order pass. Sizes are computed once per `embeds()` call and passed through as slices.
- Impact: eliminates repeated O(n) traversals inside backtracking; all size lookups become O(1).

**Size pre-filter in DFS** (`embedding.rs`)
- When searching b's nodes for a candidate image of a's root, subtrees smaller than `a.size()` are now skipped entirely during traversal (not just at the root-match point).
- Same pruning applied inside `can_embed_in_subtree` when recursing into b's children.
- Impact: reduces the number of nodes visited in b during the embedding search.

**Label multiset pre-filter** (`embedding.rs`)
- Before attempting any recursive embedding, verify that b contains at least as many occurrences of each label as a requires.
- Implemented as a single O(n + m) scan with a fixed-size counter array.
- Impact: fast rejection for structurally incompatible trees with no recursion.

**Fail-fast child ordering** (`embedding.rs`)
- Children of a node in A are sorted by decreasing subtree size before backtracking.
- Matching the hardest (largest) child first causes failures to surface earlier, pruning the backtracking tree.

**Parallel candidate scan** (`generator.rs`, `rayon`)
- Added `rayon` dependency.
- The candidate scan (finding the first valid tree at each position) now uses `par_iter().find_first()`, which processes candidates in parallel while still returning the leftmost (strategy-ordered) match.
- Preserves deterministic output.

### Tests

Added `src/tests.rs` with 25 unit tests across three modules:

- **`embedding_tests`** (13 tests): trivial cases, structural embedding, label mismatch, branch distinctness, sequence invariant verification, reflexivity.
- **`canonical_tests`** (5 tests): single nodes, chains, child sorting, nested sorting, known sequence spot-checks.
- **`generator_tests`** (7 tests): TREE(1)=1, TREE(2)=3, node budget enforcement, sequence invariant (both strategies), regression test locking the first 7 canonical forms under the largest strategy.

---

## [Unreleased] — 2026-03-13 (pass 1)

### Performance

**Benchmark: `generate --count 30 --max-nodes 9` (release build)**

| Version | Time | Notes |
|---------|------|-------|
| pass 1 (candidate caching + precomputed sizes) | 38 s | previous baseline |
| pass 2 (fingerprint pre-rejection) | **34.5 s** | ~10% further gain |

#### Changes

**Stack-allocated `TreeFingerprint`** (`src/fingerprint.rs`, new)
- Added `TreeFingerprint`: a 17-byte `Copy` struct computed per accepted tree and
  per candidate in the parallel scan. Zero heap allocations.
- Fields: `size`, `label_counts[8]`, `max_degree_per_label[8]`.
- `compatible(a, b)` is an O(1) gate before calling `embeds()`: rejects pairs
  where the widest A-node for any label exceeds the widest B-node for that label.
  This is a necessary condition for embedding (each A-child needs a distinct branch
  in B). Catches cases that pass the label-count filter but fail on degree.
- `fingerprint` stored in `SequenceEntry` so accepted trees pay the computation
  cost once; candidates compute it once per scan closure.

**Parallel inner loop: investigated, reverted**
- Tested `sequence.par_iter().any()` nested inside `candidates.par_iter().find_first()`.
- No speedup: when outer par_iter saturates the rayon thread pool, inner par_iter
  tasks are processed inline (sequentially). Documented in `BACKLOG.md` [PERF-5]
  with conditions under which it could help in the future.

### Added

- `BACKLOG.md`: documented remaining performance ideas and features, including
  GPU (investigated — not viable due to recursive backtracking structure) and
  non-viable approaches with explanations.

---

## [alpha-0.0.1] — 2026-03-13

### Added

- Initial implementation: arena-based Tree data structure, homeomorphic embedding check, BFS tree enumeration with memoization, greedy sequence generator, SVG renderer with layered layout, `clap` CLI.
- Example `.cmd` scripts in `scripts/`.
