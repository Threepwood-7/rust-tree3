//! GPU-accelerated post-acceptance sweep using CUDA (via cudarc).
//!
//! ## Two modes
//!
//! **Full-GPU mode** (requires nvcc at build time, `SWEEP_PTX_PATH` set):
//!   The CUDA kernel performs both the fingerprint gate and the full iterative
//!   homeomorphic embedding check entirely on-device.  `out[i] = 1` means
//!   accepted_tree embeds into candidate_i.
//!
//! **Fp-filter mode** (fallback, always available, built-in PTX):
//!   The CUDA kernel runs only the fingerprint compatibility check.
//!   Candidates that pass the fast GPU filter are then checked on CPU (rayon)
//!   for the actual embedding.  This still provides a meaningful speedup
//!   because the GPU fingerprint filter rejects ~70% of candidates in parallel,
//!   leaving the CPU free to run embedding only on the ~30% survivors.
//!
//! The active mode is reported at startup.
//!
//! Build with full-GPU mode:
//! ```sh
//! # With CUDA toolkit (nvcc) installed:
//! cargo build --release --features cuda
//! # Without nvcc (fp-filter fallback):
//! cargo build --release --features cuda   # still works, auto-detects
//! ```

use crate::fingerprint::TreeFingerprint;
use crate::tree::Tree;
use std::sync::atomic::AtomicBool;

// ── Flat encoding structs (CPU side, must match CUDA C / PTX exactly) ─────────

/// Fixed-size flat tree encoding.  Must stay in sync with `kernels/sweep.cu`.
///
/// Layout (112 bytes):
///   [0]       n          — number of nodes (≤ 9)
///   [1..10]   labels[9]  — label of node i
///   [10..19]  par[9]     — parent index (0xFF = root)
///   [19..28]  n_ch[9]    — number of children of node i
///   [28..37]  sz[9]      — precomputed subtree size of node i
///   [37..109] ch[9][8]   — children of node i, sorted by sz desc
///   [109..112] _pad[3]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuFlatTree {
    pub n: u8,
    pub labels: [u8; 9],
    pub par: [u8; 9],
    pub n_ch: [u8; 9],
    pub sz: [u8; 9],
    pub ch: [[u8; 8]; 9],
    pub _pad: [u8; 3],
}

/// Fixed-size flat fingerprint encoding.  Must stay in sync with `kernels/sweep.cu`.
///
/// Layout (20 bytes):
///   [0]     size
///   [1..9]  label_counts[0..8]
///   [9..17] max_deg[0..8]
///   [17..20] _pad[3]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuFlatFP {
    pub size: u8,
    pub label_counts: [u8; 8],
    pub max_deg: [u8; 8],
    pub _pad: [u8; 3],
}

impl Default for GpuFlatTree {
    fn default() -> Self {
        // Safety: all-zero is a valid bit pattern (no node has n=0 or label=0 in practice,
        // but zero-init is fine as a default/padding value).
        unsafe { std::mem::zeroed() }
    }
}

impl Default for GpuFlatFP {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// ── Encoding helpers ──────────────────────────────────────────────────────────

pub fn tree_to_flat(tree: &Tree) -> GpuFlatTree {
    let n = tree.nodes.len();
    assert!(n <= 9, "tree has {} nodes but MAX_NODES=9", n);
    let sizes = tree.all_subtree_sizes();
    let mut flat = GpuFlatTree::default();
    flat.n = n as u8;
    for i in 0..n {
        let node = &tree.nodes[i];
        flat.labels[i] = node.label as u8;
        flat.par[i] = node.parent.map(|p| p as u8).unwrap_or(0xFF);
        flat.sz[i] = sizes[i] as u8;
        let nc = node.children.len().min(8);
        flat.n_ch[i] = nc as u8;
        let mut ch_sorted = node.children[..nc].to_vec();
        ch_sorted.sort_unstable_by(|&a, &b| sizes[b].cmp(&sizes[a]));
        for (j, &c) in ch_sorted.iter().enumerate() {
            flat.ch[i][j] = c as u8;
        }
    }
    flat
}

pub fn fp_to_flat(fp: &TreeFingerprint) -> GpuFlatFP {
    GpuFlatFP {
        size: fp.size,
        label_counts: fp.label_counts,
        max_deg: fp.max_degree_per_label,
        _pad: [0; 3],
    }
}

// ── CUDA implementation ───────────────────────────────────────────────────────

#[cfg(feature = "cuda")]
mod inner {
    use super::{fp_to_flat, tree_to_flat, GpuFlatFP, GpuFlatTree};
    use crate::embedding::embeds;
    use crate::fingerprint::TreeFingerprint;
    use crate::tree::Tree;
    use cudarc::driver::{CudaContext, CudaFunction, CudaSlice, CudaStream, LaunchConfig, PushKernelArg};
    use cudarc::nvrtc::Ptx;
    use rayon::prelude::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    // Safety: both are #[repr(C)] all-u8 structs; every bit pattern is valid.
    unsafe impl cudarc::driver::DeviceRepr for GpuFlatTree {}
    unsafe impl cudarc::driver::DeviceRepr for GpuFlatFP {}

    /// The fp-only fallback PTX is embedded at compile time from the source tree.
    /// This works without nvcc — only `nvcuda.dll` (driver API) is needed.
    static FP_ONLY_PTX: &str = include_str!("../kernels/sweep_fp.ptx");

    /// The full-embedding PTX is compiled by build.rs if nvcc is found.
    /// build.rs patches the `.version` header to 8.6 (max for CUDA 12.6 driver)
    /// so the kernel loads on any driver ≥ 12.6.
    const FULL_PTX_PATH: Option<&str> = option_env!("SWEEP_PTX_PATH");

    /// Kernel source embedded for NVRTC JIT compilation at runtime.
    /// NVRTC generates PTX compatible with the installed driver regardless of
    /// which nvcc version compiled the binary.

    pub enum GpuMode {
        FullEmbed(CudaFunction),
        FpFilter(CudaFunction),
    }

    pub struct GpuSweeper {
        stream: Arc<CudaStream>,
        mode: GpuMode,
        d_b_fps: CudaSlice<GpuFlatFP>,
        /// Only populated in FullEmbed mode.
        d_b_trees: Option<CudaSlice<GpuFlatTree>>,
        d_rejected: CudaSlice<u8>,
        d_out: CudaSlice<u8>,
        n: usize,
    }

    impl GpuSweeper {
        pub fn try_new(
            entries: &[(String, Tree)],
            fingerprints: &[TreeFingerprint],
        ) -> Result<Self, String> {
            let n = entries.len();
            if n == 0 {
                return Err("empty candidate pool".to_string());
            }

            let ctx =
                CudaContext::new(0).map_err(|e| format!("CUDA device init failed: {}", e))?;
            let stream = ctx.default_stream();

            // Flat fingerprints (always needed).
            let b_fps_flat: Vec<GpuFlatFP> = fingerprints.iter().map(fp_to_flat).collect();
            let d_b_fps = stream
                .clone_htod(&b_fps_flat)
                .map_err(|e| format!("htod b_fps: {}", e))?;

            let d_rejected = stream
                .alloc_zeros::<u8>(n)
                .map_err(|e| format!("alloc rejected: {}", e))?;
            let d_out = stream
                .alloc_zeros::<u8>(n)
                .map_err(|e| format!("alloc out: {}", e))?;

            // Try the full-embed kernel (requires nvcc at build time, and a driver
            // that supports the toolkit version used to compile it).
            if let Some(ptx_path) = FULL_PTX_PATH {
                let full_result = std::fs::read_to_string(ptx_path)
                    .map_err(|e| format!("read PTX: {}", e))
                    .and_then(|ptx_src| {
                        ctx.load_module(Ptx::from_src(ptx_src))
                            .map_err(|e| format!("load_module: {:?}", e))
                    })
                    .and_then(|module| {
                        module
                            .load_function("sweep_kernel")
                            .map_err(|e| format!("load_function: {:?}", e))
                    });
                match full_result {
                    Ok(func) => {
                        let b_trees_flat: Vec<GpuFlatTree> =
                            entries.iter().map(|(_, t)| tree_to_flat(t)).collect();
                        let d_b_trees = stream
                            .clone_htod(&b_trees_flat)
                            .map_err(|e| format!("htod b_trees: {}", e))?;
                        eprintln!(
                            "  GPU mode: FULL embedding on device ({} candidates, {} MB)",
                            n,
                            (n * std::mem::size_of::<GpuFlatTree>()) / 1_048_576
                        );
                        return Ok(Self {
                            stream,
                            mode: GpuMode::FullEmbed(func),
                            d_b_fps,
                            d_b_trees: Some(d_b_trees),
                            d_rejected,
                            d_out,
                            n,
                        });
                    }
                    Err(_) => {
                        // Kernel was compiled with a newer toolkit than the installed driver
                        // supports (forward-incompatibility). Falls back to fp-filter mode.
                        // To enable full-embed mode: update the GPU driver to a version that
                        // supports the CUDA toolkit used at build time.
                    }
                }
            }

            // Fallback: fp-filter PTX (always available — hand-written sm_75 PTX 6.5).
            let ptx = Ptx::from_src(FP_ONLY_PTX.to_string());
            let module = ctx
                .load_module(ptx)
                .map_err(|e| format!("fp PTX load failed: {}", e))?;
            let func = module
                .load_function("sweep_fp_kernel")
                .map_err(|e| format!("sweep_fp_kernel not found: {}", e))?;
            eprintln!(
                "  GPU mode: fp-filter on device + CPU embedding for survivors ({} candidates)",
                n
            );
            Ok(Self {
                stream,
                mode: GpuMode::FpFilter(func),
                d_b_fps,
                d_b_trees: None,
                d_rejected,
                d_out,
                n,
            })
        }

        /// Sync the CPU rejection bitset to device memory.
        pub fn sync_rejected(&mut self, rejected: &[AtomicBool]) -> Result<(), String> {
            let host: Vec<u8> = rejected
                .iter()
                .map(|r| r.load(Ordering::Relaxed) as u8)
                .collect();
            self.stream
                .memcpy_htod(&host, &mut self.d_rejected)
                .map_err(|e| format!("sync rejected: {}", e))
        }

        /// Run the GPU sweep and update the CPU rejection bitset.
        /// Returns the count of newly rejected candidates.
        pub fn sweep(
            &mut self,
            accepted_tree: &Tree,
            accepted_fp: &TreeFingerprint,
            rejected: &[AtomicBool],
            cpu_entries: &[(String, Tree)],
        ) -> Result<usize, String> {
            let n = self.n;

            // Upload accepted tree fingerprint.
            let a_fp_flat = fp_to_flat(accepted_fp);
            let d_a_fp = self
                .stream
                .clone_htod(&[a_fp_flat])
                .map_err(|e| format!("htod a_fp: {}", e))?;

            // Sync rejection state.
            self.sync_rejected(rejected)?;

            // Zero output buffer.
            self.stream
                .memset_zeros(&mut self.d_out)
                .map_err(|e| format!("zero d_out: {}", e))?;

            let n_u32 = n as u32;
            let cfg = LaunchConfig::for_num_elems(n_u32);

            match &self.mode {
                GpuMode::FullEmbed(func) => {
                    let a_flat = tree_to_flat(accepted_tree);
                    let d_a_tree = self
                        .stream
                        .clone_htod(&[a_flat])
                        .map_err(|e| format!("htod a_tree: {}", e))?;

                    let func = func.clone();
                    let d_b_trees = self.d_b_trees.as_ref().unwrap();
                    unsafe {
                        let mut launch = self.stream.launch_builder(&func);
                        launch.arg(&d_a_tree);
                        launch.arg(d_b_trees);
                        launch.arg(&d_a_fp);
                        launch.arg(&self.d_b_fps);
                        launch.arg(&self.d_rejected);
                        launch.arg(&mut self.d_out);
                        launch.arg(&n_u32);
                        launch.launch(cfg)
                    }
                    .map_err(|e| format!("full-embed kernel launch: {}", e))?;

                    let out = self
                        .stream
                        .clone_dtoh(&self.d_out)
                        .map_err(|e| format!("dtoh out: {}", e))?;

                    let mut count = 0usize;
                    for (i, &val) in out.iter().enumerate() {
                        if val == 1 {
                            rejected[i].store(true, Ordering::Relaxed);
                            count += 1;
                        }
                    }
                    Ok(count)
                }

                GpuMode::FpFilter(func) => {
                    let func = func.clone();
                    unsafe {
                        let mut launch = self.stream.launch_builder(&func);
                        launch.arg(&d_a_fp);
                        launch.arg(&self.d_b_fps);
                        launch.arg(&self.d_rejected);
                        launch.arg(&mut self.d_out);
                        launch.arg(&n_u32);
                        launch.launch(cfg)
                    }
                    .map_err(|e| format!("fp-filter kernel launch: {}", e))?;

                    let fp_passed = self
                        .stream
                        .clone_dtoh(&self.d_out)
                        .map_err(|e| format!("dtoh fp_passed: {}", e))?;

                    // CPU: embedding check on survivors (parallel, rayon).
                    use std::sync::atomic::AtomicUsize;
                    let count = AtomicUsize::new(0);
                    cpu_entries.par_iter().enumerate().for_each(|(i, (_, cand))| {
                        if fp_passed[i] == 1 {
                            if embeds(accepted_tree, cand) {
                                rejected[i].store(true, Ordering::Relaxed);
                                count.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    });
                    Ok(count.load(Ordering::Relaxed))
                }
            }
        }

        /// Report which GPU mode is active.
        pub fn mode_description(&self) -> &'static str {
            match self.mode {
                GpuMode::FullEmbed(_) => "full GPU embedding",
                GpuMode::FpFilter(_) => "GPU fp-filter + CPU embedding",
            }
        }
    }

}

// ── Public type ───────────────────────────────────────────────────────────────

#[cfg(feature = "cuda")]
pub use inner::GpuSweeper;

/// Stub when compiled without the `cuda` feature.
#[cfg(not(feature = "cuda"))]
pub struct GpuSweeper;

#[cfg(not(feature = "cuda"))]
impl GpuSweeper {
    pub fn try_new(
        _entries: &[(String, Tree)],
        _fingerprints: &[TreeFingerprint],
    ) -> Result<Self, String> {
        Err("binary was not compiled with --features cuda".to_string())
    }

    pub fn sync_rejected(&mut self, _rejected: &[AtomicBool]) -> Result<(), String> {
        unreachable!()
    }

    pub fn sweep(
        &mut self,
        _accepted_tree: &Tree,
        _accepted_fp: &TreeFingerprint,
        _rejected: &[AtomicBool],
        _cpu_entries: &[(String, Tree)],
    ) -> Result<usize, String> {
        unreachable!("GpuSweeper::sweep called without cuda feature")
    }

    pub fn mode_description(&self) -> &'static str {
        "n/a"
    }
}
