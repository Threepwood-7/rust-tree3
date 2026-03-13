use crate::tree::Tree;

/// Check if tree `a` homeomorphically embeds into tree `b`.
///
/// The embedding is defined as: there exists an injective map f: V(A) -> V(B) such that:
/// - label(v) == label(f(v)) for all v in A  (exact label match)
/// - u is a proper ancestor of v in A => f(u) is a proper ancestor of f(v) in B
///
/// For rooted unordered trees this is equivalent to the recursive check:
/// a_node maps to b_node if labels match and each child of a_node can be
/// injectively matched to a *subtree* of a distinct child of b_node.
pub fn embeds(a: &Tree, b: &Tree) -> bool {
    let a_root = a.root;
    let a_root_label = a.nodes[a_root].label;

    // Try every node in b as a candidate image of a's root
    let mut stack = vec![b.root];
    while let Some(bn) = stack.pop() {
        if b.nodes[bn].label == a_root_label {
            if embeds_into_subtree(a, a_root, b, bn) {
                return true;
            }
        }
        for &c in &b.nodes[bn].children {
            stack.push(c);
        }
    }
    false
}

/// Check if the subtree of `a` rooted at `a_node` embeds into the subtree of `b`
/// rooted at `b_node`, with `a_node` mapping specifically to `b_node`.
///
/// Labels must match, and then all children of a_node must be injectively
/// matched to distinct children of b_node such that each a-child embeds
/// (anywhere in) the corresponding b-child's subtree.
pub fn embeds_into_subtree(a: &Tree, a_node: usize, b: &Tree, b_node: usize) -> bool {
    if a.nodes[a_node].label != b.nodes[b_node].label {
        return false;
    }

    let a_ch: Vec<usize> = a.nodes[a_node].children.clone();
    if a_ch.is_empty() {
        return true; // leaf of A always embeds once labels match
    }

    let b_ch: Vec<usize> = b.nodes[b_node].children.clone();
    if b_ch.len() < a_ch.len() {
        return false; // can't inject more children than b has
    }

    let mut used = vec![false; b_ch.len()];
    match_children(a, &a_ch, b, &b_ch, &mut used)
}

/// Try to injectively match each element of `a_children` to a distinct child
/// in `b_children` such that the a-child's subtree embeds into the b-child's subtree.
fn match_children(
    a: &Tree,
    a_children: &[usize],
    b: &Tree,
    b_children: &[usize],
    used: &mut Vec<bool>,
) -> bool {
    if a_children.is_empty() {
        return true;
    }

    let ac = a_children[0];
    let rest = &a_children[1..];

    for (i, &bc) in b_children.iter().enumerate() {
        if !used[i] && can_embed_in_subtree(a, ac, b, bc) {
            used[i] = true;
            if match_children(a, rest, b, b_children, used) {
                return true;
            }
            used[i] = false;
        }
    }
    false
}

/// Check if the subtree of `a` rooted at `a_node` can embed *somewhere*
/// in the subtree of `b` rooted at `b_node` (with a_node's image in b_node's subtree).
///
/// This differs from `embeds_into_subtree` in that the image of a_node doesn't
/// have to be b_node itself — it can be any node in b_node's subtree.
fn can_embed_in_subtree(a: &Tree, a_node: usize, b: &Tree, b_node: usize) -> bool {
    let a_label = a.nodes[a_node].label;
    // BFS/DFS over b_node's subtree
    let mut stack = vec![b_node];
    while let Some(bn) = stack.pop() {
        if b.nodes[bn].label == a_label && embeds_into_subtree(a, a_node, b, bn) {
            return true;
        }
        for &c in &b.nodes[bn].children {
            stack.push(c);
        }
    }
    false
}
