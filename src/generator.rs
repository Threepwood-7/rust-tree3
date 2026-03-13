use crate::canonical::canonicalize;
use crate::embedding::embeds;
use crate::tree::Tree;
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

    for position in 1..=count {
        let allowed_size = position.min(max_nodes);

        // Build candidate list according to strategy
        let candidates = match strategy {
            SelectionStrategy::SmallestFirst => {
                all_trees_up_to_size_smallest_first(allowed_size, k, &mut cache)
            }
            SelectionStrategy::LargestFirst => {
                all_trees_up_to_size_largest_first(allowed_size, k, &mut cache)
            }
        };

        let mut found = false;
        for (canon, tree) in candidates {
            if used_canons.contains(&canon) {
                continue;
            }

            // Check: does any previously accepted tree embed into this candidate?
            let valid = !sequence.iter().any(|entry| embeds(&entry.tree, &tree));

            if valid {
                used_canons.insert(canon.clone());
                let entry = SequenceEntry {
                    index: sequence.len() + 1,
                    tree,
                    canonical: canon,
                };
                on_found(&entry);
                sequence.push(entry);
                found = true;
                break;
            }
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
