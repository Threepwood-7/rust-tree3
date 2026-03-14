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
    /// Pick a uniformly random valid tree at each position.
    Random,
}

/// Minimal xorshift64 RNG — no external dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Ensure the state is never zero.
        Self(if seed == 0 { 0x123456789abcdef1 } else { seed })
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform random index in `0..n`.
    pub fn next_usize(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
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

    /// Pick a uniformly random non-rejected candidate.
    /// Two-pass: count live entries, then walk to the chosen offset.
    fn find_random_live(&self, rng: &mut Rng) -> Option<(usize, &String, &Tree)> {
        let live = self.live_count();
        if live == 0 {
            return None;
        }
        let target = rng.next_usize(live);
        let mut seen = 0usize;
        for (i, (canon, tree)) in self.entries.iter().enumerate() {
            if !self.rejected[i].load(Ordering::Relaxed) {
                if seen == target {
                    return Some((i, canon, tree));
                }
                seen += 1;
            }
        }
        None
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
    seed: Option<u64>,
    mut on_found: F,
) -> Vec<SequenceEntry>
where
    F: FnMut(&SequenceEntry),
{
    let mut sequence: Vec<SequenceEntry> = Vec::new();
    let mut cache: TreeCache = HashMap::new();

    // Initialise RNG for the Random strategy.
    // Seed: explicit --seed value, or mix of system time + thread id for variety.
    let mut rng = {
        let s = seed.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            let t = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdeadbeef);
            t.wrapping_mul(0x9e3779b97f4a7c15)
        });
        if matches!(strategy, SelectionStrategy::Random) {
            eprintln!("RNG seed: {}", s);
        }
        Rng::new(s)
    };

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
                // Random uses largest-first as the backing order; selection is
                // randomised at pick time, not at sort time.
                SelectionStrategy::LargestFirst | SelectionStrategy::Random => {
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

        // Scan: pick the next candidate according to strategy.
        let found = match strategy {
            SelectionStrategy::Random => pool.find_random_live(&mut rng),
            _ => pool.find_first_live(),
        };
        match found {
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

// ── Optimal exhaustive search ──────────────────────────────────────────────────

/// Recursive DFS with refcount-based rejection and backtracking.
///
/// `rejected[i]` counts how many currently-accepted trees force candidate `i`
/// to be unavailable. A candidate is usable iff `rejected[i] == 0`.
///
/// When candidate `chosen` is accepted:
///   - `rejected[chosen] += 1`   (cannot reuse the same tree)
///   - `rejected[j] += 1` for all j in `embeds_into[chosen]`  (no later tree may have
///     an earlier tree embed into it)
///
/// Backtracking is the exact mirror (all decremented).
fn dfs_optimal(
    candidates: &[(String, Tree)],
    fingerprints: &[TreeFingerprint],
    embeds_into: &[Vec<usize>],
    rejected: &mut Vec<u32>,
    sequence: &mut Vec<usize>,
    best: &mut Vec<usize>,
    max_nodes: usize,
    target: usize,
    on_new_best: &mut dyn FnMut(&[usize]),
) {
    // Upper-bound pruning: even if every live candidate is usable, can we beat `best`?
    let live: usize = rejected.iter().filter(|&&r| r == 0).count();
    if sequence.len() + live <= best.len() {
        return;
    }

    // Target reached — record if best and stop going deeper.
    if target > 0 && sequence.len() >= target {
        if sequence.len() > best.len() {
            *best = sequence.clone();
            on_new_best(best);
        }
        return;
    }

    let position = sequence.len() + 1; // 1-based position for the next tree
    let allowed_size = position.min(max_nodes);
    let mut extended = false;

    for i in 0..candidates.len() {
        if rejected[i] != 0 {
            continue;
        }
        if candidates[i].1.size() > allowed_size {
            continue; // too large for this position; may be valid later, not permanently rejected
        }

        // Tentatively accept candidate i.
        rejected[i] += 1;
        for &j in &embeds_into[i] {
            rejected[j] += 1;
        }
        sequence.push(i);
        extended = true;

        dfs_optimal(
            candidates,
            fingerprints,
            embeds_into,
            rejected,
            sequence,
            best,
            max_nodes,
            target,
            on_new_best,
        );

        // Undo.
        sequence.pop();
        rejected[i] -= 1;
        for &j in &embeds_into[i] {
            rejected[j] -= 1;
        }
    }

    // Dead end: no valid extension exists from this node. Record if new best.
    if !extended && sequence.len() > best.len() {
        *best = sequence.clone();
        on_new_best(best);
    }
}

/// Exhaustive backtracking search for the LONGEST valid TREE(k) sequence.
///
/// Unlike the greedy strategies, this tries every valid candidate at each
/// position and backtracks to explore all possibilities. This is exponential
/// time — practical only for small `--max-nodes` (≤ 6 recommended for k=3).
///
/// Precomputes `embeds_into[i]` (all j where tree_i embeds into tree_j) in
/// parallel before the DFS. This O(N²) upfront cost pays for itself because
/// each DFS step sweeps in O(|embeds_into[i]|) instead of O(N).
///
/// `count` is the target sequence length (0 = search for absolute maximum).
/// `on_new_best` is called with the full sequence each time a longer one is found.
pub fn generate_sequence_optimal<F>(
    count: usize,
    max_nodes: usize,
    k: u32,
    mut on_new_best: F,
) -> Vec<SequenceEntry>
where
    F: FnMut(&[SequenceEntry]),
{
    let mut cache: TreeCache = HashMap::new();

    eprintln!(
        "Pre-computing tree library (up to {} nodes, {} labels)...",
        max_nodes, k
    );
    for sz in 1..=max_nodes {
        let trees = all_trees_of_size_cached(sz, k, &mut cache);
        eprintln!("  Size {:2}: {:8} trees", sz, trees.len());
    }
    eprintln!("Parallel workers: {}", rayon::current_num_threads());
    eprintln!();

    // Largest-first ordering: DFS finds strong solutions early → tighter pruning.
    let all_candidates = all_trees_up_to_size_largest_first(max_nodes, k, &mut cache);
    let n = all_candidates.len();
    eprintln!("Candidate pool: {} trees.", n);

    if n > 15_000 {
        eprintln!(
            "Warning: N={} is large. The O(N²) precomputation and exponential DFS",
            n
        );
        eprintln!("may be impractical. Consider --max-nodes <= 6 for optimal strategy.");
    }

    // Fingerprints (parallel).
    let fingerprints: Vec<TreeFingerprint> = all_candidates
        .par_iter()
        .map(|(_, t)| TreeFingerprint::compute(t))
        .collect();

    // embeds_into[i] = list of j (j≠i) where tree_i homeomorphically embeds into tree_j.
    // If tree_i is placed in the sequence, every tree_j in this list is permanently
    // forbidden from appearing later.
    eprintln!("Precomputing embeds_into table (O(N²) with fingerprint gate)...");
    let embeds_into: Vec<Vec<usize>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let (_, tree_i) = &all_candidates[i];
            let fp_i = &fingerprints[i];
            (0..n)
                .filter(|&j| {
                    j != i
                        && TreeFingerprint::compatible(fp_i, &fingerprints[j])
                        && embeds(tree_i, &all_candidates[j].1)
                })
                .collect()
        })
        .collect();

    let total_edges: usize = embeds_into.iter().map(|v| v.len()).sum();
    eprintln!("embeds_into ready: {} directed pairs.", total_edges);
    eprintln!(
        "Starting optimal DFS (target: {})...",
        if count == 0 {
            "maximum".to_string()
        } else {
            count.to_string()
        }
    );
    eprintln!();

    let mut rejected = vec![0u32; n];
    let mut sequence_indices: Vec<usize> = Vec::new();
    let mut best_indices: Vec<usize> = Vec::new();

    {
        // Both candidates_ref and fps_ref are shared borrows — Rust allows them to be
        // captured by `wrapped` AND passed as parameters to `dfs_optimal` simultaneously.
        let candidates_ref: &[(String, Tree)] = &all_candidates;
        let fps_ref: &[TreeFingerprint] = &fingerprints;

        let mut wrapped = |indices: &[usize]| {
            eprintln!("  *** New best: {} trees ***", indices.len());
            let entries: Vec<SequenceEntry> = indices
                .iter()
                .enumerate()
                .map(|(pos, &idx)| {
                    let (canon, tree) = &candidates_ref[idx];
                    SequenceEntry {
                        index: pos + 1,
                        tree: tree.clone(),
                        canonical: canon.clone(),
                        fingerprint: fps_ref[idx],
                    }
                })
                .collect();
            on_new_best(&entries);
        };

        dfs_optimal(
            candidates_ref,
            fps_ref,
            &embeds_into,
            &mut rejected,
            &mut sequence_indices,
            &mut best_indices,
            max_nodes,
            count,
            &mut wrapped,
        );
    }

    eprintln!(
        "Optimal search complete. Best sequence: {} trees.",
        best_indices.len()
    );

    best_indices
        .iter()
        .enumerate()
        .map(|(pos, &idx)| {
            let (canon, tree) = &all_candidates[idx];
            SequenceEntry {
                index: pos + 1,
                tree: tree.clone(),
                canonical: canon.clone(),
                fingerprint: fingerprints[idx],
            }
        })
        .collect()
}
