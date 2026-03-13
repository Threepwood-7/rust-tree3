use crate::tree::Tree;

/// Compute the canonical string representation of a tree.
/// Format: label(child1,child2,...) where children are sorted lexicographically.
/// A leaf with label l is represented as just "l".
pub fn canonicalize(tree: &Tree) -> String {
    canon_node(tree, tree.root)
}

fn canon_node(tree: &Tree, node_idx: usize) -> String {
    let label = tree.nodes[node_idx].label;
    let children = &tree.nodes[node_idx].children;

    if children.is_empty() {
        return label.to_string();
    }

    let mut child_strs: Vec<String> = children
        .iter()
        .map(|&c| canon_node(tree, c))
        .collect();
    child_strs.sort();

    format!("{}({})", label, child_strs.join(","))
}
