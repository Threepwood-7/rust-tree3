/*
 * sweep.cu — CUDA kernel for TREE(k) post-acceptance sweep.
 *
 * For each candidate B_i in the pool, determines whether the accepted tree A
 * homeomorphically embeds into B_i.  Matches the Rust embedding.rs logic exactly.
 *
 * Tree encoding (GpuFlatTree, 112 bytes):
 *   n          : u8         number of nodes (≤ 9)
 *   labels[9]  : u8[9]      label of node i
 *   par[9]     : u8[9]      parent index; 0xFF = root
 *   n_ch[9]    : u8[9]      number of children of node i
 *   sz[9]      : u8[9]      precomputed subtree size of node i
 *   ch[9][8]   : u8[9][8]   children of node i, sorted by sz descending
 *   _pad[3]    : u8[3]
 *
 * Fingerprint encoding (GpuFlatFP, 20 bytes):
 *   size          : u8
 *   label_counts  : u8[8]
 *   max_deg       : u8[8]
 *   _pad          : u8[3]
 */

#include <stdint.h>

#define MAX_NODES 9
#define MAX_CH    8
#define MAX_LABELS 8

typedef struct {
    uint8_t n;
    uint8_t labels[MAX_NODES];
    uint8_t par[MAX_NODES];
    uint8_t n_ch[MAX_NODES];
    uint8_t sz[MAX_NODES];
    uint8_t ch[MAX_NODES][MAX_CH];
    uint8_t _pad[3];
} FlatTree;
/* static_assert: sizeof(FlatTree) == 1+9+9+9+9+72+3 = 112 */

typedef struct {
    uint8_t size;
    uint8_t label_counts[MAX_LABELS];
    uint8_t max_deg[MAX_LABELS];
    uint8_t _pad[3];
} FlatFP;
/* static_assert: sizeof(FlatFP) == 1+8+8+3 = 20 */

/* ── Fingerprint gate ──────────────────────────────────────────────────────── */

__device__ static int fp_compat(const FlatFP* a, const FlatFP* b) {
    if (a->size > b->size) return 0;
    for (int l = 1; l < MAX_LABELS; l++) {
        if (a->label_counts[l] > b->label_counts[l]) return 0;
        if (a->max_deg[l]      > b->max_deg[l])      return 0;
    }
    return 1;
}

/* ── Embedding (mirrors embedding.rs) ─────────────────────────────────────── */

/* Forward declarations (mutual recursion: do_embed ↔ can_embed_sub) */
__device__ static int do_embed(const FlatTree* a, int an,
                                const FlatTree* b, int bn);

/*
 * can_embed_sub: can the subtree of 'a' at 'an' embed *somewhere* inside
 * the subtree of 'b' at 'bn'?  DFS over b's subtree; calls do_embed.
 */
__device__ static int can_embed_sub(const FlatTree* a, int an,
                                     const FlatTree* b, int bn) {
    uint8_t a_label = a->labels[an];
    uint8_t a_sz    = a->sz[an];

    /* Explicit stack — avoids unbounded recursion for the outer DFS loop. */
    uint8_t stk[MAX_NODES];
    int top = 0;
    stk[top++] = (uint8_t)bn;

    while (top > 0) {
        int cur = (int)stk[--top];
        if (b->labels[cur] == a_label && b->sz[cur] >= a_sz) {
            if (do_embed(a, an, b, cur)) return 1;
        }
        for (int i = 0; i < (int)b->n_ch[cur]; i++) {
            int child = (int)b->ch[cur][i];
            if (b->sz[child] >= a_sz) {
                stk[top++] = (uint8_t)child;
            }
        }
    }
    return 0;
}

/*
 * match_ch: backtracking injective matching of a's children (a_ch[0..na])
 * into distinct subtrees among b's children (b_ch[0..nb]).
 *
 * 'used' is an array of nb flags indicating which b-children are taken.
 * a_ch and b_ch are pointers into a->ch[an] and b->ch[bn] arrays.
 */
__device__ static int match_ch(const FlatTree* a,
                                 const uint8_t* a_ch, int na,
                                 const FlatTree* b,
                                 const uint8_t* b_ch, int nb,
                                 uint8_t* used) {
    if (na == 0) return 1;

    int ac = (int)a_ch[0];          /* current a-child node index */

    for (int i = 0; i < nb; i++) {
        if (!used[i]) {
            int bc = (int)b_ch[i];  /* candidate b-child node index */
            if (a->sz[ac] <= b->sz[bc] &&
                can_embed_sub(a, ac, b, bc))
            {
                used[i] = 1;
                if (match_ch(a, a_ch + 1, na - 1, b, b_ch, nb, used))
                    return 1;
                used[i] = 0;
            }
        }
    }
    return 0;
}

/*
 * do_embed: check if the subtree of 'a' rooted at 'an' embeds into the
 * subtree of 'b' rooted at 'bn', with an mapped to bn.
 */
__device__ static int do_embed(const FlatTree* a, int an,
                                const FlatTree* b, int bn) {
    if (a->labels[an] != b->labels[bn]) return 0;

    int na = (int)a->n_ch[an];
    if (na == 0) return 1;          /* leaf matches once labels agree */

    int nb = (int)b->n_ch[bn];
    if (nb < na) return 0;

    /* Stack-allocated used[] — MAX_CH = 8 bytes, fits in registers */
    uint8_t used[MAX_CH] = {0};
    return match_ch(a, a->ch[an], na, b, b->ch[bn], nb, used);
}

/*
 * gpu_embeds: does tree 'a' homeomorphically embed into tree 'b'?
 *
 * Tries every node in b as a candidate root mapping for a's root (node 0),
 * skipping nodes whose subtree is too small or whose label doesn't match.
 */
__device__ static int gpu_embeds(const FlatTree* a, const FlatTree* b) {
    if (a->n > b->n) return 0;

    uint8_t a_label = a->labels[0];   /* root of a is always node 0 */
    uint8_t a_sz    = a->sz[0];

    /* DFS over b, trying each node as root mapping for a's root */
    uint8_t stk[MAX_NODES];
    int top = 0;
    stk[top++] = 0;                   /* root of b is always node 0 */

    while (top > 0) {
        int bn = (int)stk[--top];
        if (b->sz[bn] >= a_sz && b->labels[bn] == a_label) {
            if (do_embed(a, 0, b, bn)) return 1;
        }
        for (int i = 0; i < (int)b->n_ch[bn]; i++) {
            int child = (int)b->ch[bn][i];
            if (b->sz[child] >= a_sz) {
                stk[top++] = (uint8_t)child;
            }
        }
    }
    return 0;
}

/* ── Kernel ────────────────────────────────────────────────────────────────── */

/*
 * sweep_kernel: for each candidate B_i, decide if A embeds into B_i.
 *
 * Parameters:
 *   a_tree     : the single accepted tree (broadcast to all threads)
 *   b_trees    : array of N candidate trees
 *   a_fp       : fingerprint of a_tree
 *   b_fps      : fingerprints of all candidates
 *   rejected   : u8[N], 1 = already rejected (skip)
 *   out        : u8[N], output: 1 = A embeds into B_i (newly rejected)
 *   n          : number of candidates
 */
extern "C" __global__ void sweep_kernel(
    const FlatTree* __restrict__ a_tree,
    const FlatTree* __restrict__ b_trees,
    const FlatFP*   __restrict__ a_fp,
    const FlatFP*   __restrict__ b_fps,
    const uint8_t*  __restrict__ rejected,
          uint8_t*               out,
    unsigned int                 n)
{
    unsigned int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i >= n) { return; }

    /* Skip already-rejected candidates */
    if (rejected[i]) { out[i] = 0; return; }

    /* Fast fingerprint gate */
    if (!fp_compat(a_fp, &b_fps[i])) { out[i] = 0; return; }

    /* Full embedding check */
    out[i] = gpu_embeds(a_tree, &b_trees[i]) ? 1 : 0;
}
