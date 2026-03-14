# Specification for Python App: TREE(3) Initial Sequence Visualizer

## 1. Introduction

### 1.1 Purpose
The app is a Python-based tool designed to compute and visualize the initial sequence of trees contributing to the TREE(3) function from graph theory and googology. TREE(3) represents the maximum length of a sequence of rooted, vertex-labeled trees using 3 labels (1, 2, 3), where no earlier tree in the sequence is inf-embeddable into a later one. Due to the enormous size of TREE(3), the app focuses on computing and visualizing only the initial prefix of such a sequence (e.g., the first 10-20 trees), where tree sizes remain computationally feasible.

The trees will be represented using bracket notation for generation and parsing, visualized as colored SVG graphs (label 1: red, label 2: green, label 3: blue), and output as individual SVG files or a combined gallery.

### 1.2 Scope
- Compute the initial sequence by enumerating candidate trees in a canonical order and checking the inf-embedding condition.
- Limit computation to trees with a maximum number of nodes (configurable, default: 20) to prevent exponential blowup.
- Visualize each tree in the sequence as an SVG file with hierarchical layout and colored nodes.
- No full computation of TREE(3), as it is infeasibly large; focus on prefix.
- Command-line interface for running the app with parameters like sequence length or max node limit.

### 1.3 Assumptions
- Trees are rooted and ordered (children left-to-right), but canonicalized to treat as unordered via sorting in bracket notation.
- Labels: 1 (red, represented by '()'), 2 (green, '[]'), 3 (blue, '{}').
- Inf-embedding follows the standard definition: non-decreasing labels, preserves ancestor-descendant relations, and siblings map to distinct subtrees.
- Users have basic Python knowledge to run and modify the app.

## 2. Requirements

### 2.1 Functional Requirements
- **Tree Generation**: Generate all possible well-formed bracket strings (balanced with 3 bracket types) in order of increasing number of nodes (string length / 2), then lexicographical order based on character ordering: '(' < ')' < '[' < ']' < '{' < '}'.
- **Parsing**: Parse bracket strings into tree data structures.
- **Embedding Check**: Implement a function to determine if one tree inf-embeds into another.
- **Sequence Building**: Build the sequence by testing candidates against existing sequence trees.
- **Visualization**: Render each sequence tree as an SVG with colors and hierarchical layout.
- **Output**: Save SVGs to files (e.g., tree_1.svg, tree_2.svg) and print sequence bracket notations.

### 2.2 Non-Functional Requirements
- **Performance**: Feasible for trees up to 20 nodes; use memoization in embedding checks to optimize.
- **Scalability**: Configurable limits to avoid memory exhaustion.
- **Usability**: Command-line args for max sequence length, max nodes, output directory.
- **Reliability**: Validate bracket strings for balance; handle errors gracefully.

### 2.3 Technical Requirements
- **Python Version**: 3.8+
- **Libraries**:
  - `networkx`: For graph representation.
  - `matplotlib`: For drawing and SVG export.
  - No additional installs beyond these (standard for data viz).
- **Environment**: Runs on standard OS (Windows/Linux/Mac); no internet required.

## 3. Data Model

### 3.1 Tree Representation
- Trees are represented as dictionaries or a custom class `TreeNode`:
  ```python
  class TreeNode:
      def __init__(self, label: int, children: list['TreeNode'] = None):
          self.label = label  # 1, 2, or 3
          self.children = children or []  # List of TreeNode
  ```
- Bracket mapping:
  - Label 1: '()'
  - Label 2: '[]'
  - Label 3: '{}'
- Canonical string: Recursive string where children are sorted lexicographically to ensure uniqueness (treating trees as unordered).

### 3.2 Bracket String
- A string like '[(())(())]' represents a tree with root label 2, two children (each label 1 with a label 1 child).
- Validation: Must be balanced, no mismatched brackets.

## 4. Tree Generation

### 4.1 Algorithm
- Recursively generate all well-formed multi-bracket strings:
  - Base: For 1 node,: '()', '[]', '{}'
  - Recursive: For a root bracket type, insert combinations of sub-strings inside, ensuring balance.
- To avoid duplicates (since trees unordered), sort the list of child strings lexicographically before concatenating.
- Generate in batches by node count (1 to max_nodes):
  - For each node count n, generate strings of length 2*n.
- Order: Sort generated strings for each n lexicographically.
- Function signature:
  ```python
  def generate_trees(max_nodes: int) -> list[str]:
      # Return sorted list of canonical bracket strings up to max_nodes
  ```
- Use a set to deduplicate canonical forms.

## 5. Parsing and Utilities

### 5.1 Parsing Bracket to Tree
- Recursive parser:
  - Track position in string.
  - When opening bracket, create node with corresponding label.
  - Parse children until closing.
- Handle errors: Raise ValueError on mismatch.
- Function:
  ```python
  def parse_bracket(s: str) -> TreeNode:
      # Implementation as in initial code, but using TreeNode class
  ```

### 5.2 Canonical String from Tree
- Recursive: Get strings for children, sort them, concatenate inside root brackets.
- Used to normalize during generation.

## 6. Embedding Check

### 6.1 Definition
- Tree A inf-embeds into B if there exists an injective map F from A's vertices to B's such that:
  - label_B(F(v)) >= label_A(v)
  - If w is descendant of v in A, F(w) is descendant of F(v) in B.
  - For distinct children w1, w2 of v in A, path from F(w1) to F(w2) in B contains F(v) (i.e., in distinct subtrees).
- This allows edge subdivisions.

### 6.2 Algorithm
- Recursive function with memoization (dict keyed on tree hashes or serialized strings).
- def can_embed(A: TreeNode, B: TreeNode) -> bool:
  - If A is None or empty: True (base case).
  - For each potential image vertex u in B (traverse B's nodes):
    - If label(u) < label(A): continue
    - Get sub_A: list of children subtrees of A's root (sorted canonically if needed).
    - Get sub_B: list of children subtrees of u.
    - If len(sub_A) > len(sub_B): continue (not enough distinct subtrees).
    - Use backtracking to assign each sub_A[i] to a distinct sub_B[j] such that can_embed(sub_A[i], sub_B[j]).
    - If a matching exists: return True.
  - Return False.
- Base: If A has no children and label ok: True.
- Memoize calls using hash of (A.canonical_str, B.canonical_str).
- Since initial trees small (<20 nodes), backtracking feasible (subtree assignments factorial but small k).

## 7. Sequence Computation

### 7.1 Algorithm
- Initialize sequence = []
- Generate candidates in order (increasing nodes, then lex).
- For each candidate C (parse to TreeNode):
  - For each existing S in sequence:
    - If can_embed(S, C): skip C (violates condition).
  - If no violations: append C to sequence.
- Stop when sequence reaches user-specified length or candidates exceed max_nodes.
- Output bracket strings of sequence.

## 8. Visualization

### 8.1 Method
- Convert TreeNode to NetworkX DiGraph (rooted tree).
- Compute positions: Hierarchical layout (recursive: center children below parent, spaced evenly).
- Colors: {'1': 'red', '2': 'green', '3': 'blue'}
- Draw with matplotlib: node_color by label, no labels, save as SVG.
- Function:
  ```python
  def visualize_tree(tree: TreeNode, filename: str):
      # Build graph, positions, draw, save SVG
  ```
- For sequence, generate tree_i.svg for each.

## 9. Main Application

### 9.1 Execution Flow
- Parse args: max_sequence (default 10), max_nodes (default 20), output_dir (default './trees')
- Generate candidates up to max_nodes.
- Compute sequence until max_sequence or no more candidates.
- For each in sequence: visualize and save.
- Print: Sequence bracket strings and file paths.

### 9.2 Command-Line Interface
- Use argparse.
- Example: python tree_app.py --max_sequence 15 --max_nodes 25 --output_dir output

## 10. Testing and Validation

### 10.1 Unit Tests
- Test parsing: Valid/invalid brackets.
- Test generation: First few match known (e.g., '()', '[]', '{}', '[()]', etc.).
- Test embedding: Known pairs (e.g., single node embeds into any >= label).
- Test sequence: First 5-10 match literature (e.g., '{}', '[[]]', '([][])', '[()()()]', '[(())(())]').

### 10.2 Integration Tests
- Run for small max, verify SVGs generated.

## 11. Limitations and Future Enhancements
- Computation halts at large trees due to enumeration explosion and embedding checks.
- Assumes ordered brackets; if unordered needed, enhance canonicalization.
- Enhancements: Interactive GUI, parallelize checks, extend to TREE(k) for k>3.
- References: Kruskal's Tree Theorem (Wikipedia), Friedman’s writings, Numberphile videos for initial sequences.