use crate::tree::Tree;

/// Check if tree `a` homeomorphically embeds into tree `b`.
///
/// The embedding is defined as: there exists an injective map f: V(A) -> V(B) such that:
/// - label(v) == label(f(v)) for all v in A  (exact label match)
/// - u is a proper ancestor of v in A => f(u) is a proper ancestor of f(v) in B
pub fn embeds(a: &Tree, b: &Tree) -> bool {
    // Pre-filter 1: a can't embed into b if a has strictly more nodes.
    if a.nodes.len() > b.nodes.len() {
        return false;
    }

    // Pre-filter 2: for each label, a must not require more occurrences than b provides.
    if !label_multiset_fits(a, b) {
        return false;
    }

    // Precompute subtree sizes once per tree — avoids O(n) calls inside the hot path.
    let a_sizes = a.all_subtree_sizes();
    let b_sizes = b.all_subtree_sizes();

    let a_root = a.root;
    let a_root_label = a.nodes[a_root].label;
    let a_total = a_sizes[a_root];

    // Try every node in b as a candidate image of a's root.
    // Skip b-nodes whose subtree is too small to contain a.
    let mut stack = vec![b.root];
    while let Some(bn) = stack.pop() {
        if b_sizes[bn] >= a_total
            && b.nodes[bn].label == a_root_label
            && embeds_with_sizes(a, a_root, &a_sizes, b, bn, &b_sizes)
        {
            return true;
        }
        for &c in &b.nodes[bn].children {
            if b_sizes[c] >= a_total {
                stack.push(c);
            }
        }
    }
    false
}

/// Returns true if every label that appears in `a` appears at least as many
/// times in `b`. Supports up to 31 distinct label values.
fn label_multiset_fits(a: &Tree, b: &Tree) -> bool {
    let mut counts = [0i32; 32];
    for node in &b.nodes {
        counts[node.label as usize] += 1;
    }
    for node in &a.nodes {
        let c = &mut counts[node.label as usize];
        *c -= 1;
        if *c < 0 {
            return false;
        }
    }
    true
}

/// Check if the subtree of `a` at `a_node` embeds into the subtree of `b` at `b_node`,
/// with `a_node` mapping to `b_node`. Uses precomputed subtree sizes.
fn embeds_with_sizes(
    a: &Tree,
    a_node: usize,
    a_sizes: &[usize],
    b: &Tree,
    b_node: usize,
    b_sizes: &[usize],
) -> bool {
    if a.nodes[a_node].label != b.nodes[b_node].label {
        return false;
    }

    let a_ch = &a.nodes[a_node].children;
    if a_ch.is_empty() {
        return true; // leaf embeds once labels match
    }

    let b_ch = &b.nodes[b_node].children;
    if b_ch.len() < a_ch.len() {
        return false;
    }

    // Sort a-children largest-first for fail-fast pruning during backtracking.
    let mut a_ch_sorted: Vec<usize> = a_ch.clone();
    a_ch_sorted.sort_unstable_by(|&x, &y| a_sizes[y].cmp(&a_sizes[x]));

    let mut used = vec![false; b_ch.len()];
    match_children(a, &a_ch_sorted, a_sizes, b, b_ch, b_sizes, &mut used)
}

/// Backtracking injective matching: each a-child maps to a distinct b-child subtree.
fn match_children(
    a: &Tree,
    a_children: &[usize],
    a_sizes: &[usize],
    b: &Tree,
    b_children: &[usize],
    b_sizes: &[usize],
    used: &mut Vec<bool>,
) -> bool {
    if a_children.is_empty() {
        return true;
    }

    let ac = a_children[0];
    let rest = &a_children[1..];
    let ac_size = a_sizes[ac];

    for (i, &bc) in b_children.iter().enumerate() {
        if !used[i]
            && b_sizes[bc] >= ac_size
            && can_embed_in_subtree(a, ac, a_sizes, b, bc, b_sizes)
        {
            used[i] = true;
            if match_children(a, rest, a_sizes, b, b_children, b_sizes, used) {
                return true;
            }
            used[i] = false;
        }
    }
    false
}

/// Check if the subtree of `a` at `a_node` can embed *somewhere* in the subtree of `b` at `b_node`.
fn can_embed_in_subtree(
    a: &Tree,
    a_node: usize,
    a_sizes: &[usize],
    b: &Tree,
    b_node: usize,
    b_sizes: &[usize],
) -> bool {
    let a_label = a.nodes[a_node].label;
    let a_size = a_sizes[a_node];
    let mut stack = vec![b_node];
    while let Some(bn) = stack.pop() {
        if b.nodes[bn].label == a_label
            && b_sizes[bn] >= a_size
            && embeds_with_sizes(a, a_node, a_sizes, b, bn, b_sizes)
        {
            return true;
        }
        for &c in &b.nodes[bn].children {
            if b_sizes[c] >= a_size {
                stack.push(c);
            }
        }
    }
    false
}
