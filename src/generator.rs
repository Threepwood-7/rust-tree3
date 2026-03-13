use crate::canonical::canonicalize;
use crate::embedding::embeds;
use crate::fingerprint::TreeFingerprint;
use crate::memlock;
use crate::tree::Tree;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Cache for all_trees_of_size results: (size, k) -> Vec<(canonical, tree)>
type TreeCache = HashMap<(usize, u32), Vec<(String, Tree)>>;

/// Generate all distinct (up to canonical form) rooted labeled trees of
/// exactly `size` nodes using labels from `1..=k`, with memoization.
pub fn all_trees_of_size_cached(
    size: usize,
    k: u32,
    cache: &mut TreeCache,
) -> Vec<(String, Tree)> {
    let key = (size, k);
    if cache.contains_key(&key) {
        return cache[&key].clone();
    }

    let result = compute_trees_of_size(size, k, cache);
    cache.insert(key, result.clone());
    result
}

fn compute_trees_of_size(
    size: usize,
    k: u32,
    cache: &mut TreeCache,
) -> Vec<(String, Tree)> {
    if size == 0 {
        return Vec::new();
    }
    if size == 1 {
        return (1..=k)
            .map(|label| {
                let tree = Tree::new_single_node(label);
                let canon = canonicalize(&tree);
                (canon, tree)
            })
            .collect();
    }

    // Enumerate all child-subtree combinations once (sequential: reads cache for sizes < n).
    let combos: Vec<Vec<Tree>> = partitions_into_subtrees_cached(size - 1, k, cache);

    // Build trees for every (root_label, combo) pair in parallel.
    let pairs: Vec<(u32, &Vec<Tree>)> = (1..=k)
        .flat_map(|rl: u32| combos.iter().map(move |c| (rl, c)))
        .collect();

    let mut result: Vec<(String, Tree)> = pairs
        .par_iter()
        .map(|(root_label, combo)| {
            let tree = Tree::from_root_and_children(*root_label, combo);
            let canon = canonicalize(&tree);
            (canon, tree)
        })
        .collect();

    result.par_sort_unstable_by(|a, b| a.0.cmp(&b.0));
    result.dedup_by(|a, b| a.0 == b.0);
    result
}

/// Generate all ordered lists of subtrees (sorted by canonical form) summing to `remaining`.
fn partitions_into_subtrees_cached(
    remaining: usize,
    k: u32,
    cache: &mut TreeCache,
) -> Vec<Vec<Tree>> {
    if remaining == 0 {
        return vec![vec![]];
    }
    let mut result = Vec::new();
    gen_combos_cached(remaining, k, &String::new(), &mut result, cache);
    result
}

fn gen_combos_cached(
    remaining: usize,
    k: u32,
    min_canon: &str,
    result: &mut Vec<Vec<Tree>>,
    cache: &mut TreeCache,
) {
    if remaining == 0 {
        result.push(vec![]);
        return;
    }

    for sz in 1..=remaining {
        let subtrees = all_trees_of_size_cached(sz, k, cache);
        for (canon, subtree) in subtrees {
            if canon.as_str() >= min_canon {
                let mut sub_combos = Vec::new();
                gen_combos_cached(remaining - sz, k, &canon, &mut sub_combos, cache);
                for mut combo in sub_combos {
                    combo.insert(0, subtree.clone());
                    result.push(combo);
                }
            }
        }
    }
}

/// Get all trees up to `max_size` nodes, largest-first.
fn all_trees_up_to_size_largest_first(
    max_size: usize,
    k: u32,
    cache: &mut TreeCache,
) -> Vec<(String, Tree)> {
    let mut result = Vec::new();
    for sz in 1..=max_size {
        result.extend(all_trees_of_size_cached(sz, k, cache));
    }
    result.par_sort_unstable_by(|a, b| {
        b.1.size()
            .cmp(&a.1.size())
            .then_with(|| a.0.cmp(&b.0))
    });
    result
}

/// Get all trees up to `max_size` nodes, smallest-first.
fn all_trees_up_to_size_smallest_first(
    max_size: usize,
    k: u32,
    cache: &mut TreeCache,
) -> Vec<(String, Tree)> {
    let mut result = Vec::new();
    for sz in 1..=max_size {
        result.extend(all_trees_of_size_cached(sz, k, cache));
    }
    result.par_sort_unstable_by(|a, b| {
        a.1.size()
            .cmp(&b.1.size())
            .then_with(|| a.0.cmp(&b.0))
    });
    result
}

/// How to order candidate trees for selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStrategy {
    /// Pick the smallest valid tree first (canonical order within each size).
    SmallestFirst,
    /// Pick the largest valid tree first (largest trees used as early as possible).
    LargestFirst,
}

/// Result of a sequence generation step.
pub struct SequenceEntry {
    pub index: usize,
    pub tree: Tree,
    pub canonical: String,
    /// Precomputed fingerprint for the post-acceptance sweep and future checks.
    pub fingerprint: TreeFingerprint,
}

// ── CandidatePool ─────────────────────────────────────────────────────────────

/// A strategy-sorted list of candidate trees with pre-stored fingerprints and a
/// permanent rejection bitset.
///
/// All mutation uses interior mutability (`AtomicBool`) so `&self` is sufficient
/// for both the parallel post-acceptance sweep and the parallel scan — no locking
/// required at the pool level.
///
/// The two flat arrays (`fingerprints`, `rejected`) are pinned into physical RAM
/// at construction time via `memlock::try_lock_in_ram`. They are the hottest
/// data accessed at every scan and every sweep and must not be paged to swap.
struct CandidatePool {
    /// Strategy-sorted candidates: (canonical_form, tree).
    entries: Vec<(String, Tree)>,
    /// Pre-stored fingerprint for each entry (parallel flat array).
    /// Pinned in physical RAM: avoids recomputation at every sweep/scan.
    fingerprints: Vec<TreeFingerprint>,
    /// Permanent rejection flag per entry.
    /// `true` ⟹ this candidate is banned for all future positions.
    /// Pinned in physical RAM: the scan reads this for every candidate at every position.
    rejected: Vec<AtomicBool>,
}

impl CandidatePool {
    /// Build a pool from a strategy-sorted candidate list.
    /// Fingerprints are computed in parallel; flat arrays are then locked in RAM.
    fn new(sorted: Vec<(String, Tree)>) -> Self {
        let n = sorted.len();

        let fingerprints: Vec<TreeFingerprint> = sorted
            .par_iter()
            .map(|(_, t)| TreeFingerprint::compute(t))
            .collect();

        let rejected: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();

        memlock::try_lock_in_ram(&fingerprints, "fingerprints");
        memlock::try_lock_in_ram(&rejected, "rejected-bitset");

        Self { entries: sorted, fingerprints, rejected }
    }

    fn live_count(&self) -> usize {
        self.rejected
            .par_iter()
            .filter(|r| !r.load(Ordering::Relaxed))
            .count()
    }

    /// Mark the candidate at index `i` as permanently rejected.
    fn reject(&self, i: usize) {
        self.rejected[i].store(true, Ordering::Relaxed);
    }

    /// Post-acceptance sweep: for every non-rejected candidate C, if
    /// `embeds(accepted_tree, C)` — i.e., the just-accepted tree embeds into C —
    /// mark C as permanently rejected (it can never appear after this position).
    ///
    /// Uses the pre-stored fingerprint as an O(1) gate before the recursive check.
    /// Returns the count of newly rejected candidates.
    fn sweep(&self, accepted_tree: &Tree, accepted_fp: &TreeFingerprint) -> usize {
        let count = AtomicUsize::new(0);
        self.entries.par_iter().enumerate().for_each(|(i, (_, cand_tree))| {
            if self.rejected[i].load(Ordering::Relaxed) {
                return;
            }
            if !TreeFingerprint::compatible(accepted_fp, &self.fingerprints[i]) {
                return;
            }
            if embeds(accepted_tree, cand_tree) {
                self.rejected[i].store(true, Ordering::Relaxed);
                count.fetch_add(1, Ordering::Relaxed);
            }
        });
        count.load(Ordering::Relaxed)
    }

    /// Find the first non-rejected candidate in strategy order.
    /// `par_iter().find_first()` preserves sort order while scanning in parallel.
    fn find_first_live(&self) -> Option<(usize, &String, &Tree)> {
        self.entries
            .par_iter()
            .enumerate()
            .find_first(|(i, _)| !self.rejected[*i].load(Ordering::Relaxed))
            .map(|(i, (canon, tree))| (i, canon, tree))
    }
}

// ── Sequence generation ───────────────────────────────────────────────────────

/// Generate a valid TREE(k) sequence up to `count` trees.
///
/// TREE(k) rule: T₁, T₂, … where the i-th tree has at most i nodes,
/// and no Tᵢ homeomorphically embeds into any Tⱼ for j > i.
///
/// `max_nodes` is a hard cap on tree size.
/// `strategy` controls greedy selection order.
pub fn generate_sequence<F>(
    count: usize,
    max_nodes: usize,
    k: u32,
    strategy: SelectionStrategy,
    mut on_found: F,
) -> Vec<SequenceEntry>
where
    F: FnMut(&SequenceEntry),
{
    let mut sequence: Vec<SequenceEntry> = Vec::new();
    let mut cache: TreeCache = HashMap::new();

    // Pre-warm: enumerate all trees up to max_nodes once.
    eprintln!("Pre-computing tree library (up to {} nodes, {} labels)...", max_nodes, k);
    for sz in 1..=max_nodes {
        let trees = all_trees_of_size_cached(sz, k, &mut cache);
        eprintln!("  Size {:2}: {:8} trees", sz, trees.len());
    }
    eprintln!("Parallel workers: {}", rayon::current_num_threads());
    eprintln!();

    // Pool cache: rebuilt only when `allowed_size` grows (positions 1..max_nodes).
    // Once `allowed_size` plateaus at max_nodes, the pool is reused indefinitely;
    // only the `rejected` bitset changes (shrinking after each accepted tree).
    let mut pool_cache: Option<(usize, CandidatePool)> = None;

    for position in 1..=count {
        let allowed_size = position.min(max_nodes);

        let rebuild = pool_cache
            .as_ref()
            .map_or(true, |(cached_size, _)| *cached_size != allowed_size);

        if rebuild {
            let sorted = match strategy {
                SelectionStrategy::SmallestFirst => {
                    all_trees_up_to_size_smallest_first(allowed_size, k, &mut cache)
                }
                SelectionStrategy::LargestFirst => {
                    all_trees_up_to_size_largest_first(allowed_size, k, &mut cache)
                }
            };
            let n = sorted.len();
            eprintln!(
                "Building candidate pool (max size ≤ {}, {} candidates)...",
                allowed_size, n
            );
            let new_pool = CandidatePool::new(sorted);

            // Replay: sweep every previously accepted tree to initialize the rejection
            // bitset for this new pool. Self-embeddings (a tree into itself) are caught
            // here, so no separate `used_canons` set is needed.
            if !sequence.is_empty() {
                eprintln!(
                    "  Initializing rejections from {} accepted trees...",
                    sequence.len()
                );
                for entry in &sequence {
                    let n_swept = new_pool.sweep(&entry.tree, &entry.fingerprint);
                    eprintln!(
                        "    T{}: permanently rejected {} candidates",
                        entry.index, n_swept
                    );
                }
                eprintln!("  Pool ready: {} live of {}", new_pool.live_count(), n);
            }

            pool_cache = Some((allowed_size, new_pool));
        }

        let (_, ref pool) = pool_cache.as_ref().unwrap();

        // Scan: find the first non-rejected candidate in strategy order.
        // All embedding work for previous accepted trees was done during their sweeps,
        // so this reduces to an O(N) pass over the `rejected` bitset in physical RAM.
        match pool.find_first_live() {
            None => {
                eprintln!(
                    "Note: sequence ended at position {} (no valid tree with <= {} nodes found).",
                    position, allowed_size
                );
                break;
            }
            Some((idx, canon, tree)) => {
                let fingerprint = TreeFingerprint::compute(tree);
                let canon_owned = canon.clone();
                let tree_owned = tree.clone();

                // Mark accepted entry as permanently rejected (each tree used at most once).
                pool.reject(idx);

                let entry = SequenceEntry {
                    index: sequence.len() + 1,
                    tree: tree_owned,
                    canonical: canon_owned,
                    fingerprint,
                };
                on_found(&entry);

                // Post-acceptance sweep: prune all candidates this tree embeds into.
                let n_swept = pool.sweep(&entry.tree, &entry.fingerprint);
                eprintln!(
                    "  Position {:3}: T{} ({} nodes), swept {} new rejections, {} live remaining",
                    position,
                    entry.index,
                    entry.tree.size(),
                    n_swept,
                    pool.live_count(),
                );

                sequence.push(entry);
            }
        }
    }

    sequence
}
