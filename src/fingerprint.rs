use crate::tree::Tree;

/// Precomputed per-tree fingerprint for fast embedding pre-rejection.
///
/// Entirely stack-allocated (no heap). `compatible(a, b)` returns `false` if
/// `a` definitely cannot homeomorphically embed into `b`:
///
/// 1. **Size**: `a.size > b.size`
/// 2. **Label counts**: any label appears more times in A than B
/// 3. **Max degree per label**: for each label l, the maximum child-count of
///    any node with label l in A must be ≤ that in B. A wider node in A cannot
///    map to any narrower node in B (each A-child needs a distinct B-branch).
///
/// Supports up to 8 distinct labels (sufficient for k ≤ 7).
#[derive(Debug, Clone, Copy)]
pub struct TreeFingerprint {
    pub size: u8,
    pub label_counts: [u8; 8],
    pub max_degree_per_label: [u8; 8],
}

impl TreeFingerprint {
    #[inline]
    pub fn compute(tree: &Tree) -> Self {
        let mut label_counts = [0u8; 8];
        let mut max_degree_per_label = [0u8; 8];

        for node in &tree.nodes {
            let l = (node.label as usize).min(7);
            label_counts[l] = label_counts[l].saturating_add(1);
            let d = node.children.len() as u8;
            if d > max_degree_per_label[l] {
                max_degree_per_label[l] = d;
            }
        }

        TreeFingerprint {
            size: tree.nodes.len() as u8,
            label_counts,
            max_degree_per_label,
        }
    }

    /// Returns `false` if `a` definitely cannot embed into `b`.
    /// Returns `true` if embedding is still plausible.
    #[inline]
    pub fn compatible(a: &Self, b: &Self) -> bool {
        if a.size > b.size {
            return false;
        }
        for l in 1..8usize {
            if a.label_counts[l] > b.label_counts[l] {
                return false;
            }
            // The widest A-node with label l needs a B-node with label l
            // that has at least as many children.
            if a.max_degree_per_label[l] > b.max_degree_per_label[l] {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Tree;

    fn leaf(label: u32) -> Tree { Tree::new_single_node(label) }
    fn tree(root: u32, children: &[Tree]) -> Tree {
        Tree::from_root_and_children(root, children)
    }

    #[test]
    fn same_tree_compatible_with_itself() {
        let t = tree(2, &[leaf(3), leaf(3)]);
        let fp = TreeFingerprint::compute(&t);
        assert!(TreeFingerprint::compatible(&fp, &fp));
    }

    #[test]
    fn larger_rejected_by_size() {
        let big = tree(1, &[leaf(1), leaf(1)]);
        let small = leaf(1);
        let fp_big = TreeFingerprint::compute(&big);
        let fp_small = TreeFingerprint::compute(&small);
        assert!(!TreeFingerprint::compatible(&fp_big, &fp_small));
    }

    #[test]
    fn missing_label_rejected() {
        let a = tree(1, &[leaf(2)]);
        let b = tree(1, &[leaf(3)]);
        let fp_a = TreeFingerprint::compute(&a);
        let fp_b = TreeFingerprint::compute(&b);
        assert!(!TreeFingerprint::compatible(&fp_a, &fp_b));
    }

    #[test]
    fn max_degree_rejects_too_wide() {
        // A has a label-1 node with 3 children; B's only label-1 node has 2.
        let a = tree(1, &[leaf(2), leaf(2), leaf(2)]);
        let b = tree(1, &[leaf(2), leaf(2)]);
        let fp_a = TreeFingerprint::compute(&a);
        let fp_b = TreeFingerprint::compute(&b);
        assert!(!TreeFingerprint::compatible(&fp_a, &fp_b));
    }

    #[test]
    fn max_degree_passes_wider_b() {
        let a = tree(1, &[leaf(2), leaf(2)]);
        let b = tree(1, &[leaf(2), leaf(2), leaf(2)]);
        let fp_a = TreeFingerprint::compute(&a);
        let fp_b = TreeFingerprint::compute(&b);
        assert!(TreeFingerprint::compatible(&fp_a, &fp_b));
    }
}
