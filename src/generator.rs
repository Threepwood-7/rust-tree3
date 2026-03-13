use crate::canonical::canonicalize;
use crate::embedding::embeds;
use crate::fingerprint::TreeFingerprint;
use crate::tree::Tree;
use rayon::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;

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

    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for root_label in 1..=k {
        let children_combos = partitions_into_subtrees_cached(size - 1, k, cache);
        for combo in children_combos {
            let tree = Tree::from_root_and_children(root_label, &combo);
            let canon = canonicalize(&tree);
            if seen.insert(canon.clone()) {
                result.push((canon, tree));
            }
        }
    }

    // Sort for determinism
    result.sort_by(|a, b| a.0.cmp(&b.0));
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
    // Sort: largest size first, then canonical for ties
    result.sort_by(|a, b| {
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
    result.sort_by(|a, b| {
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
    /// Precomputed fingerprint for fast embedding pre-rejection in future checks.
    pub fingerprint: TreeFingerprint,
}

/// Generate a valid TREE(k) sequence up to `count` trees.
///
/// TREE(k) rule: T1, T2, ... where the i-th tree has at most i nodes,
/// and no Ti homeomorphically embeds into any Tj for j > i.
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
    let mut used_canons: HashSet<String> = HashSet::new();
    let mut cache: TreeCache = HashMap::new();

    // Pre-warm cache for all sizes up to max_nodes
    eprintln!("Pre-computing tree library (up to {} nodes, {} labels)...", max_nodes, k);
    let mut total_trees = 0usize;
    for sz in 1..=max_nodes {
        let trees = all_trees_of_size_cached(sz, k, &mut cache);
        total_trees += trees.len();
        eprintln!("  Size {:2}: {:6} trees", sz, trees.len());
    }
    eprintln!("Total candidate trees: {}", total_trees);
    eprintln!();

    // Cache the sorted candidate list to avoid re-sorting millions of trees
    // at every position once allowed_size stops growing (i.e. once position >= max_nodes).
    let mut candidates_cache: Option<(usize, Vec<(String, Tree)>)> = None;

    for position in 1..=count {
        let allowed_size = position.min(max_nodes);

        // Reuse the sorted candidate list when allowed_size hasn't changed.
        let rebuild = candidates_cache
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
            candidates_cache = Some((allowed_size, sorted));
        }

        let candidates = candidates_cache.as_ref().unwrap().1.as_slice();

        // Parallel scan: find the first candidate (in strategy order) that is valid.
        // `find_first` processes in parallel but returns the leftmost match,
        // preserving deterministic ordering.
        //
        // For each candidate:
        //   1. Compute its fingerprint once.
        //   2. Use fingerprint pre-rejection against every accepted tree before
        //      falling back to the full backtracking embeds() check.
        //   3. The inner sequence check also runs in parallel (par_iter().any()).
        let found_item = candidates
            .par_iter()
            .find_first(|(canon, candidate_tree)| {
                if used_canons.contains(canon) {
                    return false;
                }
                let cand_fp = TreeFingerprint::compute(candidate_tree);
                // Does any previously accepted tree embed into this candidate?
                // Inner loop is sequential: outer par_iter already saturates all
                // threads, so nested par_iter adds overhead without more parallelism.
                !sequence.iter().any(|entry| {
                    // Fast fingerprint gate before the expensive backtracking check.
                    TreeFingerprint::compatible(&entry.fingerprint, &cand_fp)
                        && embeds(&entry.tree, candidate_tree)
                })
            });

        let found = found_item.is_some();
        if let Some((canon, tree)) = found_item {
            let fingerprint = TreeFingerprint::compute(tree);
            used_canons.insert(canon.clone());
            let entry = SequenceEntry {
                index: sequence.len() + 1,
                tree: tree.clone(),
                canonical: canon.clone(),
                fingerprint,
            };
            on_found(&entry);
            sequence.push(entry);
        }

        if !found {
            eprintln!(
                "Note: sequence ended at position {} (no valid tree with <= {} nodes found).",
                position, allowed_size
            );
            break;
        }
    }

    sequence
}
