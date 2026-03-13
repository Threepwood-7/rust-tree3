# Changelog

All notable changes to this project will be documented in this file.

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
