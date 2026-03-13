use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub label: u32,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: usize,
}

#[allow(dead_code)]
impl Tree {
    /// Create a tree with a single node (the root) with given label.
    pub fn new_single_node(label: u32) -> Self {
        Tree {
            nodes: vec![Node {
                label,
                children: Vec::new(),
                parent: None,
            }],
            root: 0,
        }
    }

    /// Add a child node with the given label under parent_idx.
    /// Returns the index of the new child node.
    pub fn add_child(&mut self, parent_idx: usize, label: u32) -> usize {
        let child_idx = self.nodes.len();
        self.nodes.push(Node {
            label,
            children: Vec::new(),
            parent: Some(parent_idx),
        });
        self.nodes[parent_idx].children.push(child_idx);
        child_idx
    }

    /// Number of nodes in the tree.
    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// Depth of a node (root = 0).
    pub fn depth(&self, node_idx: usize) -> usize {
        let mut depth = 0;
        let mut current = node_idx;
        while let Some(p) = self.nodes[current].parent {
            depth += 1;
            current = p;
        }
        depth
    }

    /// Maximum depth of any node.
    pub fn max_depth(&self) -> usize {
        (0..self.nodes.len())
            .map(|i| self.depth(i))
            .max()
            .unwrap_or(0)
    }

    /// Get all nodes in the subtree rooted at node_idx (including node_idx itself).
    pub fn subtree_nodes(&self, node_idx: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut stack = vec![node_idx];
        while let Some(n) = stack.pop() {
            result.push(n);
            for &c in &self.nodes[n].children {
                stack.push(c);
            }
        }
        result
    }

    /// Label of a node.
    pub fn label(&self, node_idx: usize) -> u32 {
        self.nodes[node_idx].label
    }

    /// Number of nodes in the subtree rooted at node_idx.
    pub fn subtree_size(&self, node_idx: usize) -> usize {
        let mut count = 0;
        let mut stack = vec![node_idx];
        while let Some(n) = stack.pop() {
            count += 1;
            for &c in &self.nodes[n].children {
                stack.push(c);
            }
        }
        count
    }

    /// Precompute subtree sizes for all nodes in O(n).
    /// Returns a Vec<usize> indexed by node index.
    pub fn all_subtree_sizes(&self) -> Vec<usize> {
        let n = self.nodes.len();
        let mut sizes = vec![1usize; n];
        // Pre-order traversal to get node order, then accumulate in reverse.
        let mut order = Vec::with_capacity(n);
        let mut stack = vec![self.root];
        while let Some(node) = stack.pop() {
            order.push(node);
            for &c in &self.nodes[node].children {
                stack.push(c);
            }
        }
        for &node in order.iter().rev() {
            for &c in &self.nodes[node].children {
                sizes[node] += sizes[c];
            }
        }
        sizes
    }

    /// Build a tree from a root label and a list of child subtrees.
    /// This clones and re-indexes the child subtrees.
    pub fn from_root_and_children(root_label: u32, children: &[Tree]) -> Self {
        let mut tree = Tree::new_single_node(root_label);
        for child in children {
            tree.graft(0, child);
        }
        tree
    }

    /// Graft a copy of `other` tree as a child of `parent_idx` in self.
    pub fn graft(&mut self, parent_idx: usize, other: &Tree) {
        let offset = self.nodes.len();
        // Clone all nodes from other, adjusting indices
        for (i, node) in other.nodes.iter().enumerate() {
            let new_parent = if i == other.root {
                Some(parent_idx)
            } else {
                node.parent.map(|p| p + offset)
            };
            self.nodes.push(Node {
                label: node.label,
                children: node.children.iter().map(|&c| c + offset).collect(),
                parent: new_parent,
            });
        }
        let child_root = other.root + offset;
        self.nodes[parent_idx].children.push(child_root);
    }
}
