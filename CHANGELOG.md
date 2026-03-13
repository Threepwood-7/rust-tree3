# Changelog

All notable changes to this project will be documented in this file.

---

## [Unreleased] — 2026-03-13

### Performance

**Benchmark: `generate --count 30 --max-nodes 9` (release build)**

| Version | Time |
|---------|------|
| alpha-0.0.1 (baseline) | ~129 s |
| optimized | ~38 s |
| **Speedup** | **~3.3×** |

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

## [alpha-0.0.1] — 2026-03-13

### Added

- Initial implementation: arena-based Tree data structure, homeomorphic embedding check, BFS tree enumeration with memoization, greedy sequence generator, SVG renderer with layered layout, `clap` CLI.
- Example `.cmd` scripts in `scripts/`.
