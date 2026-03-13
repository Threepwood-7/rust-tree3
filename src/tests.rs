//! Test suite comparing against known-correct results for embedding,
//! canonicalization, and sequence generation.

#[cfg(test)]
mod embedding_tests {
    use crate::embedding::embeds;
    use crate::tree::Tree;

    // ── Helpers ────────────────────────────────────────────────────────────────

    /// Single node with the given label.
    fn leaf(label: u32) -> Tree {
        Tree::new_single_node(label)
    }

    /// Build a tree from a root label and child subtrees.
    fn tree(root: u32, children: &[Tree]) -> Tree {
        Tree::from_root_and_children(root, children)
    }

    // ── Trivial cases ──────────────────────────────────────────────────────────

    #[test]
    fn leaf_embeds_into_itself() {
        let a = leaf(1);
        assert!(embeds(&a, &a));
    }

    #[test]
    fn leaf_does_not_embed_different_label() {
        let a = leaf(1);
        let b = leaf(2);
        assert!(!embeds(&a, &b));
        assert!(!embeds(&b, &a));
    }

    #[test]
    fn leaf_embeds_into_any_tree_with_matching_label() {
        let a = leaf(2);
        // b is  2(3,3,3)
        let b = tree(2, &[leaf(3), leaf(3), leaf(3)]);
        assert!(embeds(&a, &b));
    }

    #[test]
    fn larger_tree_cannot_embed_into_smaller() {
        let a = tree(1, &[leaf(1), leaf(1)]);
        let b = leaf(1);
        assert!(!embeds(&a, &b));
    }

    // ── Basic structural embedding ─────────────────────────────────────────────

    #[test]
    fn chain_embeds_into_longer_chain() {
        // a = 1(2)   b = 1(2(3))
        let a = tree(1, &[leaf(2)]);
        let b = tree(1, &[tree(2, &[leaf(3)])]);
        assert!(embeds(&a, &b));
    }

    #[test]
    fn chain_does_not_embed_wrong_label() {
        // a = 1(2)   b = 1(3)  — label mismatch on child
        let a = tree(1, &[leaf(2)]);
        let b = tree(1, &[leaf(3)]);
        assert!(!embeds(&a, &b));
    }

    #[test]
    fn branching_embeds_into_wider_tree() {
        // a = 1(2,2)   b = 1(2,2,2)
        let a = tree(1, &[leaf(2), leaf(2)]);
        let b = tree(1, &[leaf(2), leaf(2), leaf(2)]);
        assert!(embeds(&a, &b));
    }

    #[test]
    fn wider_tree_does_not_embed_into_narrower() {
        // a = 1(2,2,2)   b = 1(2,2)
        let a = tree(1, &[leaf(2), leaf(2), leaf(2)]);
        let b = tree(1, &[leaf(2), leaf(2)]);
        assert!(!embeds(&a, &b));
    }

    #[test]
    fn subtree_root_can_embed_anywhere_not_just_root() {
        // a = 2(3)
        // b = 1(2(3))   — a's root maps to b's child, not b's root
        let a = tree(2, &[leaf(3)]);
        let b = tree(1, &[tree(2, &[leaf(3)])]);
        assert!(embeds(&a, &b));
    }

    // ── Children must map to *distinct* branches ───────────────────────────────

    #[test]
    fn two_children_need_two_distinct_branches() {
        // a = 1(2,2)   b = 1(2)   — b has only one branch for label 2
        let a = tree(1, &[leaf(2), leaf(2)]);
        let b = tree(1, &[leaf(2)]);
        assert!(!embeds(&a, &b));
    }

    #[test]
    fn children_use_distinct_branches_deep() {
        // a = 3(2(3),2(3))
        // b = 3(2(3),2(3),1)  — extra child doesn't break it
        let a23 = tree(2, &[leaf(3)]);
        let a = tree(3, &[a23.clone(), a23.clone()]);
        let b = tree(3, &[a23.clone(), a23.clone(), leaf(1)]);
        assert!(embeds(&a, &b));
    }

    // ── Known TREE sequence pairs ──────────────────────────────────────────────
    // These are the exact trees produced by `generate --count 7 --labels 3 --max-nodes 8 --strategy largest`
    // Verified by inspection and cross-checked with TREE(2).

    fn known_tree(index: usize) -> Tree {
        // T1=1  T2=2(2)  T3=2(3(3))  T4=2(3,3,3)
        // T5=3(2(3),2(3))  T6=3(2(3),3(2,2))  T7=3(2(3),3(2),3(2))
        match index {
            1 => leaf(1),
            2 => tree(2, &[leaf(2)]),
            3 => tree(2, &[tree(3, &[leaf(3)])]),
            4 => tree(2, &[leaf(3), leaf(3), leaf(3)]),
            5 => tree(3, &[tree(2, &[leaf(3)]), tree(2, &[leaf(3)])]),
            6 => tree(3, &[tree(2, &[leaf(3)]), tree(3, &[leaf(2), leaf(2)])]),
            7 => tree(
                3,
                &[
                    tree(2, &[leaf(3)]),
                    tree(3, &[leaf(2)]),
                    tree(3, &[leaf(2)]),
                ],
            ),
            _ => panic!("unknown index"),
        }
    }

    /// No earlier tree in the sequence may embed into any later one.
    #[test]
    fn sequence_no_earlier_embeds_into_later() {
        let trees: Vec<Tree> = (1..=7).map(known_tree).collect();
        for i in 0..trees.len() {
            for j in (i + 1)..trees.len() {
                assert!(
                    !embeds(&trees[i], &trees[j]),
                    "T{} should NOT embed into T{}",
                    i + 1,
                    j + 1
                );
            }
        }
    }

    /// Reflexivity: every tree embeds into itself.
    #[test]
    fn every_known_tree_embeds_into_itself() {
        for i in 1..=7 {
            let t = known_tree(i);
            assert!(embeds(&t, &t), "T{} should embed into itself", i);
        }
    }
}

// ── Canonicalization tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod canonical_tests {
    use crate::canonical::canonicalize;
    use crate::tree::Tree;

    fn leaf(label: u32) -> Tree {
        Tree::new_single_node(label)
    }
    fn tree(root: u32, children: &[Tree]) -> Tree {
        Tree::from_root_and_children(root, children)
    }

    #[test]
    fn single_node() {
        assert_eq!(canonicalize(&leaf(1)), "1");
        assert_eq!(canonicalize(&leaf(3)), "3");
    }

    #[test]
    fn simple_chain() {
        let t = tree(2, &[leaf(3)]);
        assert_eq!(canonicalize(&t), "2(3)");
    }

    #[test]
    fn children_are_sorted() {
        // Build with children in reverse canonical order; canonical form must sort them.
        let t_ba = tree(1, &[leaf(3), leaf(2)]);
        let t_ab = tree(1, &[leaf(2), leaf(3)]);
        assert_eq!(canonicalize(&t_ba), canonicalize(&t_ab));
        assert_eq!(canonicalize(&t_ab), "1(2,3)");
    }

    #[test]
    fn nested_children_sorted() {
        // 2(3(3),1)  and  2(1,3(3))  must produce the same canonical form
        let a = tree(2, &[tree(3, &[leaf(3)]), leaf(1)]);
        let b = tree(2, &[leaf(1), tree(3, &[leaf(3)])]);
        assert_eq!(canonicalize(&a), canonicalize(&b));
        assert_eq!(canonicalize(&a), "2(1,3(3))");
    }

    #[test]
    fn known_sequence_canonicals() {
        // Spot-check the canonical forms of the first 7 trees in the known sequence.
        let cases: &[(u32, &[&str], &str)] = &[
            // (root_label, children_canonicals_desc, expected_full_canonical)
        ];
        // Directly verify expected strings:
        let checks: &[(&str, Tree)] = &[
            ("1",              Tree::new_single_node(1)),
            ("2(2)",           Tree::from_root_and_children(2, &[Tree::new_single_node(2)])),
            ("2(3(3))",        Tree::from_root_and_children(2, &[Tree::from_root_and_children(3, &[Tree::new_single_node(3)])])),
            ("2(3,3,3)",       Tree::from_root_and_children(2, &[Tree::new_single_node(3), Tree::new_single_node(3), Tree::new_single_node(3)])),
        ];
        let _ = cases; // suppress unused warning
        for (expected, tree) in checks {
            assert_eq!(canonicalize(tree), *expected);
        }
    }
}

// ── Sequence generation tests ──────────────────────────────────────────────────

#[cfg(test)]
mod generator_tests {
    use crate::generator::{generate_sequence, SelectionStrategy};

    /// TREE(1) = 1: only label 1, sequence must terminate after the first tree.
    #[test]
    fn tree1_length_is_1() {
        let seq = generate_sequence(10, 10, 1, SelectionStrategy::LargestFirst, |_| {});
        assert_eq!(seq.len(), 1, "TREE(1) sequence must have exactly 1 tree");
        assert_eq!(seq[0].canonical, "1");
    }

    /// TREE(2) = 3: labels {1,2}, known exact value.
    #[test]
    fn tree2_length_is_3() {
        let seq = generate_sequence(10, 10, 2, SelectionStrategy::LargestFirst, |_| {});
        assert_eq!(seq.len(), 3, "TREE(2) sequence must have exactly 3 trees");
    }

    /// The i-th tree must have at most i nodes (enforced by the node budget rule).
    #[test]
    fn node_budget_respected() {
        let seq = generate_sequence(7, 8, 3, SelectionStrategy::LargestFirst, |_| {});
        for entry in &seq {
            assert!(
                entry.tree.size() <= entry.index,
                "T{} has {} nodes, expected <= {}",
                entry.index, entry.tree.size(), entry.index
            );
        }
    }

    /// First tree must always be a single-node tree (1 node at position 1).
    #[test]
    fn first_tree_is_single_node() {
        let seq = generate_sequence(5, 8, 3, SelectionStrategy::LargestFirst, |_| {});
        assert!(!seq.is_empty());
        assert_eq!(seq[0].tree.size(), 1);
    }

    /// No tree in the sequence may embed into any later tree (the core invariant).
    #[test]
    fn sequence_invariant_largest_strategy() {
        use crate::embedding::embeds;
        let seq = generate_sequence(10, 8, 3, SelectionStrategy::LargestFirst, |_| {});
        for i in 0..seq.len() {
            for j in (i + 1)..seq.len() {
                assert!(
                    !embeds(&seq[i].tree, &seq[j].tree),
                    "invariant violated: T{} embeds into T{}",
                    i + 1, j + 1
                );
            }
        }
    }

    #[test]
    fn sequence_invariant_smallest_strategy() {
        use crate::embedding::embeds;
        let seq = generate_sequence(10, 8, 3, SelectionStrategy::SmallestFirst, |_| {});
        for i in 0..seq.len() {
            for j in (i + 1)..seq.len() {
                assert!(
                    !embeds(&seq[i].tree, &seq[j].tree),
                    "invariant violated: T{} embeds into T{} (smallest strategy)",
                    i + 1, j + 1
                );
            }
        }
    }

    /// Known exact canonical forms for first 7 trees under largest strategy.
    /// These act as a regression test: any change to the algorithm that alters
    /// output must be an intentional, reviewed change.
    #[test]
    fn known_sequence_regression_largest() {
        let expected = [
            "1",
            "2(2)",
            "2(3(3))",
            "2(3,3,3)",
            "3(2(3),2(3))",
            "3(2(3),3(2,2))",
            "3(2(3),3(2),3(2))",
        ];
        let seq = generate_sequence(7, 8, 3, SelectionStrategy::LargestFirst, |_| {});
        assert_eq!(seq.len(), expected.len());
        for (i, (entry, exp)) in seq.iter().zip(expected.iter()).enumerate() {
            assert_eq!(
                entry.canonical, *exp,
                "T{} mismatch: got '{}', expected '{}'",
                i + 1, entry.canonical, exp
            );
        }
    }
}
