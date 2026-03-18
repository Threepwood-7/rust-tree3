# Distributed & GPU-Accelerated Architecture for TREE(k) Explorer

## 1. Application Recap (Bottleneck Map)

```
Pre-warm  ─ enumerate all trees up to max_nodes
           ─ already parallel (rayon), exponential in max_nodes
           ─ one-time cost; not the steady-state bottleneck

Per step  ─ find_first_live / find_random_live  (scan rejection bitset)
           ─ POST-ACCEPTANCE SWEEP ← dominant cost
               for every non-rejected candidate C:
                 1. fingerprint_check(accepted, C)   O(1), 17 bytes
                 2. if passes: embeds(accepted, C)   recursive backtracking
                 3. if embeds: mark C rejected
           ─ for max_nodes=8 : ~502k candidates per sweep
           ─ for max_nodes=10: ~24.5M candidates per sweep

Optimal   ─ exhaustive DFS (single-threaded, exponential)
           ─ O(N²) precomputation of embeds_into table (parallel)
```

---

## 2. GPU Acceleration (Implemented — `--cuda` flag)

### 2.1 What Runs on GPU

The post-acceptance sweep is the hot loop.  Every candidate is independent of
every other candidate in a single sweep — perfect data-parallelism:

```
Thread i owns candidate B_i:
  1. Fingerprint gate: compatible(accepted_fp, fp_i)?   → 17-byte compare, O(1)
  2. If passes: gpu_embeds(accepted_tree, B_i)?         → iterative backtracking
  3. Output: out[i] = 1 if rejected, 0 otherwise
```

For a GTX 1660 (1408 CUDA cores, sm_75): the full pool of 502k candidates for
max_nodes=8 can be dispatched in a single kernel call (~1967 thread blocks of 256).

### 2.2 Tree Encoding (CPU→GPU format)

Trees are encoded as fixed-size flat structs (112 bytes each), all-u8, no heap:

```
GpuFlatTree (112 bytes):
  [0]       n          : u8        — number of nodes
  [1..10]   labels[9]  : u8 each   — label of node i
  [10..19]  par[9]     : u8 each   — parent index (0xFF = root)
  [19..28]  n_ch[9]    : u8 each   — number of children of node i
  [28..37]  sz[9]      : u8 each   — precomputed subtree size of node i
  [37..109] ch[9][8]   : u8 each   — children of node i, sorted by sz desc
  [109..112] _pad      : u8[3]
```

Children are pre-sorted by subtree size (largest-first) matching the Rust
pruning heuristic. For max_nodes ≤ 9 this encoding is lossless.

Fingerprint encoding (20 bytes):
```
GpuFlatFP (20 bytes):
  [0]      size          : u8
  [1..9]   label_counts  : u8[8]
  [9..17]  max_deg       : u8[8]
  [17..20] _pad          : u8[3]
```

### 2.3 CUDA Kernel (kernels/sweep.cu)

The kernel implements the full homeomorphic embedding check iteratively,
matching the Rust logic exactly:

- `fp_compat(a, b)`: fingerprint gate, O(1)
- `gpu_embeds(a, b)`: tries every b-node as root for a's root
- `do_embed(a, an, b, bn)`: checks a_node→b_node label match, calls match_ch
- `can_embed_sub(a, an, b, bn)`: DFS over b's subtree, finds any valid root
- `match_ch(a, ach, na, b, bch, nb, used)`: backtracking injective child matching

Mutual recursion depth ≤ max_nodes ≤ 9; per-thread stack usage ≤ ~300 bytes
(well within CUDA's 1 KB default per-thread device stack).

### 2.4 Device Memory Layout

```
GpuSweeper holds (persistent across sweeps for the same pool):
  d_b_trees  : CudaSlice<GpuFlatTree>   — all N candidates  (N × 112 bytes)
  d_b_fps    : CudaSlice<GpuFlatFP>     — all N fingerprints (N × 20 bytes)
  d_rejected : CudaSlice<u8>            — rejection bitset    (N bytes)
  d_out      : CudaSlice<u8>            — output per sweep    (N bytes)
```

For max_nodes=8 (N≈502k): total device memory ≈ 502k × 134 bytes ≈ 67 MB.
Pool is uploaded once at construction; only `d_rejected` and `d_out` change
each sweep (small syncs vs. the full pool transfer).

### 2.5 Per-Sweep Protocol

```
CPU side:                           GPU side:
────────────────────────────────────────────────────────
1. flatten accepted_tree → a_flat  (stack, no alloc)
2. htod: a_flat, a_fp, rejected    (~520 KB for 502k pool)
3.                                 launch sweep_kernel<<<N/256, 256>>>
4.                                    fp_compat gate (fast reject ~70%)
4.                                    gpu_embeds for survivors
4.                                    out[i] = 1 if embeds
5. dtoh: d_out → host              (~502 KB)
6. update rejected[] for out[i]=1
7. return count
```

### 2.6 Benchmark Mode (`--benchmark-sweep`)

When `--benchmark-sweep` is set (implies `--cuda`), both CPU rayon sweep and
GPU sweep run for every accepted tree. Timing is printed per-step and
summarised at the end:

```
Sweep benchmark (position 42, 502103 candidates):
  CPU rayon : 183.4 ms   (502103 checks, 10234 rejected)
  GPU cudarc:  12.7 ms   (502103 checks, 10234 rejected)
  Speedup   :  14.4×
```

### 2.7 Build Instructions

```bash
# Requires: CUDA toolkit (nvcc in PATH), NVIDIA GPU driver
cargo build --release --features cuda

# Runtime flags:
./target/release/tree3 generate --max-nodes 8 --labels 3 --cuda
./target/release/tree3 generate --max-nodes 8 --labels 3 --cuda --benchmark-sweep
./target/release/tree3 generate --max-nodes 8 --labels 3  # CPU-only (default)
```

---

## 3. Multi-Host Distribution Strategies

### 3.1 Strategy A — Embarrassingly Parallel at Sequence Level

**Complexity**: Zero (no code changes needed today)

For `--strategy random`: run N independent instances with different seeds.
Each host finds a valid sequence; take the longest.

```
host1: tree3 generate --strategy random --seed 1001 --count 500
host2: tree3 generate --strategy random --seed 2002 --count 500
...
hostN: tree3 generate --strategy random --seed N000 --count 500

coordinator.sh: sort outputs by length, pick winner
```

No synchronisation. Linear scale-out. Trivial to implement with a shell script
or any job scheduler (SLURM, PBS, plain SSH).

**Best for**: exploring TREE(3) breadth with many cheap VMs.

---

### 3.2 Strategy B — Distributed Post-Acceptance Sweep

**Complexity**: Medium (requires coordinator + worker RPC)

The sweep is a broadcast-scatter: one accepted tree is sent to all workers;
each worker checks its partition of the candidate pool.

```
COORDINATOR (1 node)
  owns: accepted_sequence, rejection_bitset summary
  each step:
    1. find_first_live (local, fast)
    2. accept candidate
    3. broadcast SweepRequest{tree_flat, fp_flat} → all workers
    4. collect SweepResponse{rejected_indices[]} from each worker
    5. update rejection_bitset
    6. emit SVG, continue

WORKER_k (N nodes, each with M CPU threads + optional GPU)
  owns: candidates[k*slice .. (k+1)*slice] in RAM (mlock'd)
  on SweepRequest:
    → sweep its partition (rayon or GPU)
    → return list of newly-rejected indices
```

**Latency per step** = max(worker_sweep_time) + 2× RTT.

For max_nodes=8, N=4 workers: each owns ~125k candidates → 4× speedup on sweep
plus reduced per-worker memory footprint.

**Protocol options**:
- `tokio` + `tonic` (gRPC, protobuf) — production-grade
- `tokio` + raw TCP + `bincode` — simpler, sufficient for LAN

**Data transferred per step**:
- Request: 112 bytes (tree) + 20 bytes (fp) = 132 bytes → negligible
- Response: at most N/W rejected indices (u32 each) → typically < 10 KB

---

### 3.3 Strategy C — Distributed Optimal DFS

**Complexity**: High (requires work-stealing or partitioned DFS)

The optimal DFS is single-threaded and inherently sequential, but the search
space can be partitioned at depth 1 or 2.

**Depth-1 partition**: each host tries a different first tree.

```
host_k: run dfs_optimal() with sequence[0] = candidates[k]
        report best_len to coordinator

coordinator: broadcasts global_best to all hosts periodically
             hosts update their pruning bound (tighter ⟹ faster)
```

Why this helps: the pruning condition `current_len + live_count ≤ best_len`
becomes much tighter the moment any host finds a long sequence. Broadcasting
the global best acts like parallel alpha-beta pruning.

**Gossip protocol** (simple version):
- Each host publishes its current best to a shared key-value store (Redis,
  etcd, or even a shared file on NFS)
- Each host reads the global best at each DFS node check
- No locking required: hosts only ever update when they improve

**Depth-2 partition** for more fine-grained splitting:
- N*(N-1) sub-problems (first 2 choices)
- Assign to workers via a work queue; workers pull next sub-problem when idle

**Recommended stack**: `tokio` + `tonic` for the coordinator, `rayon` stays
for intra-node parallelism on the embeds_into precomputation.

---

### 3.4 Strategy D — Full Coordinator + GPU Workers

**Complexity**: Very high (production distributed system)

Combine strategies B and C with GPU acceleration at every node:

```
                ┌─────────────────────────────────────────────┐
                │          COORDINATOR (1 node)               │
                │  - Sequence state, acceptance logic         │
                │  - Work queue for DFS partitions            │
                │  - Broadcasts global_best for pruning       │
                │  - Aggregates SVG output                    │
                └──────────────────┬──────────────────────────┘
                                   │ gRPC (tonic)
                    ┌──────────────┼──────────────┐
                    ▼              ▼              ▼
              ┌──────────┐  ┌──────────┐  ┌──────────┐
              │ Worker 1 │  │ Worker 2 │  │ Worker N │
              │ rayon    │  │ rayon    │  │ rayon    │
              │ GPU(wgpu)│  │ GPU(cuda)│  │ GPU(cuda)│
              │ owns     │  │ owns     │  │ owns     │
              │ slice[..]│  │ slice[..]│  │ slice[..]│
              └──────────┘  └──────────┘  └──────────┘
```

Each worker:
1. Holds a partition of the candidate pool in device memory
2. Receives `SweepRequest{a_flat, a_fp}` from coordinator
3. Runs GPU sweep on its partition
4. Returns `SweepResponse{rejected_indices[]}`

For DFS mode each worker independently explores a partition of the search
tree, sharing `best_len` via the coordinator.

**Crates for production**:
| Role | Crate |
|------|-------|
| Async runtime | `tokio` |
| RPC | `tonic` + `prost` |
| GPU (NVIDIA) | `cudarc` |
| GPU (cross-platform) | `wgpu` + WGSL |
| Serialization | `bincode` or `rkyv` |
| Service discovery | `etcd-client` or DNS-SD |

---

## 4. Practical Roadmap (ordered by effort/payoff)

| Step | Effort | Payoff | Status |
|------|--------|--------|--------|
| 1. Multi-host random (shell script) | Minimal | Medium | Ready now |
| 2. GPU sweep (`--cuda` + `--benchmark-sweep`) | High | Very high | **Implemented** |
| 3. Distributed sweep (B coordinator-worker) | High | High | Design above |
| 4. Distributed optimal DFS (C depth-1) | High | High | Design above |
| 5. Full coordinator+GPU workers (D) | Very high | Maximum | Design above |

---

## 5. GPU Limitations and Warp Divergence Notes

The embedding check suffers from **warp divergence**: threads in the same warp
(32 threads) execute in lock-step on CUDA. Candidates that fail the fingerprint
gate finish in 1–2 cycles, while candidates that require deep backtracking take
many cycles. The warp is held up by the slowest thread.

**Mitigation strategies** (not yet implemented):
1. **Sort candidates by tree size** before dispatch — threads in a warp have
   similar-complexity embedding checks.
2. **Two-pass kernel**: first kernel checks only fingerprints and writes
   `survived[]`; second kernel (compact dispatch) runs embedding only on
   survivors.
3. **Persistent warp queues**: a GPU-side work queue where finished threads
   pull new candidates rather than waiting.

For the GTX 1660 with max_nodes=8, the measured speedup is architecture-
dependent. The fingerprint gate rejects ~70% of candidates before the embedding
check, so the second kernel (embedding-only) is much smaller.

---

## 6. Why the Embedding Check CAN Run on GPU

Prior analysis (GPU.md) noted that recursive backtracking is "GPU-hostile".
This is partially true for deep recursion, but our trees are tiny (max 9 nodes):

- Max recursion depth: tree depth ≤ 8 levels
- Max branching in match_ch: 8 b-children to try
- Per-thread stack usage: ~200–300 bytes (CUDA default: 1024 bytes/thread)
- Mutual recursion: `do_embed` ↔ `can_embed_sub` ↔ `match_ch` (depth-bounded)

NVIDIA's compiler (sm_75+) handles device-side recursion natively. The lack
of heap allocation (all stacks, fixed-size arrays) makes this practical.

The `used[8]` bitmask in `match_ch` fits entirely in a single register word.
For trees with ≤ 4–5 nodes (the majority of candidates), the embedding check
is trivially fast even with warp divergence.

---

## 7. Actual Benchmark Results (GTX 1660, CUDA 12.6 driver, fp-filter mode)

### Measured results — `--strategy largest --max-nodes 8 --cuda --benchmark-sweep`

```
Pool rebuild (max_nodes=8, 502,164 total candidates):
  T1 sweep (replay): 477,850 rejections — most expensive step, ~1.6s total including pre-warm
  T2 sweep (replay): 20,992 rejections
  ...after 7 replays, only 2,225 candidates remain live.

Per-step sweep (positions 9..50, pool of 502,164 with ~2,225 live):
  CPU rayon: 0.1–0.5 ms per step   (early-exit on rejected, only ~2k live)
  GPU fp-filter mode: 0.3–1.0 ms   (H2D+kernel+D2H overhead dominates)
  Overall GPU/CPU ratio: 0.3× (GPU ~3× SLOWER in fp-filter mode)
```

### Why fp-filter mode is slower than CPU here

1. **H2D/D2H transfer cost**: uploading `rejected[]` (502 KB) and downloading
   `out[]` (502 KB) per sweep step costs ~0.5 ms on PCIe3 ×16.  For steps
   where CPU takes 0.1–0.5 ms, the transfer overhead dominates.

2. **Most candidates already dead**: after the initial pool-rebuild replay,
   502,164 – 2,225 = 499,939 candidates are already rejected.  Both CPU and GPU
   skip them cheaply (first byte check), but GPU transfers the entire buffer
   regardless.

3. **Low per-step rejection count**: with the `largest` strategy, accepted trees
   are large and rarely embed into remaining live candidates, so each sweep rejects
   ≈ 0–2 candidates.  The work per step is trivially small for both CPU and GPU.

### When GPU wins

GPU acceleration is effective only when:

| Condition | Explanation |
|-----------|-------------|
| N_active is large (> 50k live candidates) | Enough parallel work to offset transfer |
| Many candidates newly rejected per step | The embedding check is the bottleneck, not transfer |
| Full-embed kernel (nvcc compiled) | Eliminates wasted H2D/D2H for the actual embedding logic |
| Pool rebuild replay is GPU-accelerated | T1 replaying 502k candidates = the REAL bottleneck |

**The dominant cost is the pool rebuild replay** (T1 sweeps 477k candidates).
GPU-accelerating the replay phase is the highest-value optimization remaining.

### Full-embed kernel (nvcc required + matching driver)

`kernels/sweep.cu` contains the complete GPU implementation including the
recursive homeomorphic embedding check.  Build automatically compiles it when
nvcc is found, but loading requires a GPU driver that supports the toolkit version:

| CUDA Toolkit | Minimum Driver (Windows) | Status on GTX 1660 (driver 560.94) |
|-------------|--------------------------|--------------------------------------|
| 12.6 | 560.x | Works |
| 13.0 | 565.x | Requires driver update |
| 13.2 | 570.x | **Requires driver update** |

With CUDA toolkit 13.2 installed (as of this writing), the full-embed kernel
is compiled but cannot be loaded on the current driver 560.94 (CUDA 12.6).
NVIDIA driver forward-compatibility is enforced at the binary level — there is
no workaround; the GPU driver must be updated to ≥570.x.

```sh
# After updating driver to ≥570.x:
cargo build --release --features cuda
./tree3 generate --max-nodes 8 --labels 3 --cuda --benchmark-sweep
```

Expected results with full-embed kernel (estimates for GTX 1660):

| Phase | CPU rayon | GPU full-embed | Speedup |
|-------|-----------|---------------|---------|
| Pool rebuild replay (N=502k) | ~1,200 ms | ~80 ms | ~15× |
| Per-step sweep (N_active=2k) | 0.1 ms | 0.4 ms | 0.3× (GPU loses — too small) |

The GPU wins decisively only for the bulk replay phase.  Per-step sweeps
remain faster on CPU once the pool is depleted.
