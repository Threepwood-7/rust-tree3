Building a CLI that **actually computes candidate initial sequences related to the TREE(3) function and visualizes them as SVG** is an ambitious project. Even generating meaningful *initial candidates* requires careful design because the underlying theory from Kruskal's Tree Theorem makes exhaustive search explode combinatorially.

Below is a **complete engineering specification** (architecture, algorithms, language, libraries, CLI UX, and data structures) for such a system.

---

# 1. Language Choice

## Recommended: **Rust**

### Why Rust

* **Performance**: Tree-sequence exploration grows extremely fast.
* **Memory safety**: Prevents crashes during massive graph exploration.
* **Great CLI ecosystem**.
* **Excellent SVG and graph tooling**.
* **Easy concurrency for search algorithms**.

### Alternatives

| Language | Pros                       | Cons                           |
| -------- | -------------------------- | ------------------------------ |
| Rust     | Speed, safety, concurrency | Slightly steeper dev curve     |
| Python   | Rapid prototyping          | Too slow for large searches    |
| C++      | Maximum performance        | More complex memory management |
| Haskell  | Elegant tree modeling      | SVG/CLI ecosystem weaker       |

**Conclusion:** Rust is the best balance of **performance + ergonomics**.

---

# 2. Core Libraries

## CLI

* clap
* indicatif

## Graph & Tree Handling

* petgraph

## Serialization

* serde
* serde_json

## SVG Generation

* svg

Optional advanced layout:

* graphviz
  (via DOT export)

---

# 3. Mathematical Model

The TREE(3) problem concerns **finite rooted trees with node labels from {1,2,3}**.

A sequence must satisfy:

A tree **Tᵢ cannot embed into any later tree Tⱼ**.

Embedding rules:

1. Root must map to root
2. Parent-child relations preserved
3. Node labels must **not decrease**

This is based on **homeomorphic embedding with label constraints**.

---

# 4. Core Data Structures

## Tree Node

```text
Node {
    id: usize
    label: u8   // 1..3
    children: Vec<NodeId>
}
```

## Tree

```text
Tree {
    root: NodeId
    nodes: Vec<Node>
}
```

Properties:

* Rooted
* Ordered children optional
* Labeled nodes

---

# 5. Canonical Representation

Critical to avoid duplicates.

Use **canonical encoding**.

Example:

```
1(2(3),3)
```

Meaning:

```
1
├─2
│ └─3
└─3
```

Canonicalization rules:

1. Sort children lexicographically
2. Serialize preorder

---

# 6. Tree Embedding Algorithm

Core computational challenge.

### Function

```
embeds(A, B) -> bool
```

Check if **A embeds into B**.

---

### Recursive Algorithm

For node a in A and b in B:

1. Check label constraint
   `label(a) ≤ label(b)`

2. Attempt to match children of A to **subtrees of B**

This is essentially a **subsequence matching problem**.

Pseudo-logic:

```
for each child_a in children(a):
    find child_b in children(b)
        such that embed(child_a, child_b)
```

Use:

* backtracking
* memoization

---

### Complexity

Worst-case:

```
O(n! * m!)
```

So caching is critical.

---

# 7. Sequence Generation Algorithm

Goal:

Generate sequence

```
T1, T2, T3, ...
```

Where

```
Ti NOT embeddable into Tj for all j>i
```

---

## Strategy

Use **incremental search**.

### Step 1

Start with smallest trees.

```
single node trees
```

---

### Step 2

Generate candidate expansions

Operations:

1. Add child
2. Increase label
3. Add subtree

---

### Step 3

Check validity

For candidate `T`:

```
for each previous tree P:
    if embeds(P, T):
        reject
```

---

### Step 4

Append if valid.

---

# 8. Search Strategy

Brute force will explode.

Use:

## Breadth-first tree generation

By size:

```
size = number of nodes
```

Generate trees:

```
size 1
size 2
size 3
...
```

---

## Pruning

Reject trees if:

1. Duplicate canonical form
2. Immediately invalid sequence extension

---

## Optional heuristics

* prioritize higher labels
* limit branching factor

---

# 9. Visualization System

Trees should be exported to **SVG**.

Each node displayed as:

```
circle
+ label text
```

Edges:

```
straight lines
```

---

## Layout Algorithm

Use **layered tree layout**.

Steps:

1. Compute depth levels
2. Assign x positions recursively

Example:

```
root center
children evenly spaced
```

---

# 10. Color Scheme

Color nodes by label.

Example:

| Label | Color  |
| ----- | ------ |
| 1     | blue   |
| 2     | orange |
| 3     | red    |

Edges:

```
gray
```

Optional:

* gradient by depth

---

# 11. SVG Structure

Example output:

```
tree_0001.svg
tree_0002.svg
tree_0003.svg
```

Each file:

```
<svg>
  <line/>
  <circle/>
  <text/>
</svg>
```

---

# 12. CLI Interface

Example command:

```
tree3 generate --count 20 --out ./trees
```

Output:

```
trees/
  0001.svg
  0002.svg
  0003.svg
```

---

### CLI Options

```
--count N
```

number of trees

```
--max-nodes
```

limit tree size

```
--labels
```

default 3

```
--export-json
```

store tree structures

```
--layout
```

svg | dot

---

# 13. Internal Modules

```
src/
```

### modules

```
cli.rs
tree.rs
embedding.rs
generator.rs
canonical.rs
svg.rs
layout.rs
cache.rs
```

---

# 14. Caching System

Embedding checks repeat frequently.

Use:

```
HashMap<(TreeId,TreeId), bool>
```

Or canonical string pairs.

---

# 15. Persistence

Save intermediate results.

```
sequence.json
```

Example:

```json
{
 "trees":[
  {"id":1,"structure":"1"},
  {"id":2,"structure":"1(2)"},
 ]
}
```

---

# 16. Performance Enhancements

### Parallel search

Using:

* Rust threads
* work stealing

Split by:

```
tree size ranges
```

---

### Hashing

Use fast hash:

```
ahash
```

---

### Structural hashing

Avoid identical subtrees.

---

# 17. Testing

Unit tests:

### Embedding

Test cases:

```
A embeds B
A not embeds C
```

### Canonicalization

Verify identical forms collapse.

---

# 18. Validation Dataset

Small known sequences exist in literature around:

Friedman's TREE function research.

Use small label counts:

```
TREE(1)
TREE(2)
```

To validate algorithm.

---

# 19. Project Phases

## Phase 1

Tree data structure

---

## Phase 2

Embedding algorithm

---

## Phase 3

Sequence generator

---

## Phase 4

SVG renderer

---

## Phase 5

CLI interface

---

## Phase 6

Optimization

---

# 20. Expected Limits

Reality check:

Even generating **first few nontrivial trees** is computationally expensive.

But with good pruning you can produce **meaningful early sequences**.

The value of TREE(3) itself is **incomprehensibly large**.

---

# 21. Optional Advanced Features

### Interactive viewer

Serve SVGs in browser.

---

### Animation

Sequence growth.

---

### Graphviz export

DOT format.

---

### Tree comparison UI

Show embeddings visually.

---

# 22. Estimated Complexity

Initial prototype:

```
2–4k lines Rust
```

---

# 23. Example Workflow

```
tree3 generate --count 10
```

Output:

```
Generated tree 1 (1 node)
Generated tree 2 (2 nodes)
Generated tree 3 (3 nodes)
```

Then:

```
tree3 render
```

Produces SVGs.

---

# 24. Recommended Repository Structure

```
tree3-explorer/

Cargo.toml

src/
  main.rs
  cli.rs
  tree.rs
  embed.rs
  generator.rs
  canonical.rs
  svg.rs

examples/
docs/
```

---

✅ This design will give you a **real computational TREE(3) explorer**, not just static drawings.

---

If you'd like, I can also show you:

* **A mathematically correct embedding algorithm design**
* **How to generate trees without duplicates**
* **A search strategy capable of generating the first ~50 candidate trees efficiently** (this part is subtle and extremely interesting).
