fn main() {
    if std::env::var("CARGO_FEATURE_CUDA").is_ok() {
        compile_cuda_sweep();
    }
}

/// Search for MSVC cl.exe host compiler (needed by nvcc on Windows).
/// Returns the directory containing cl.exe, or None if not found.
fn find_msvc_bin() -> Option<String> {
    let roots = [
        r"C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Tools\MSVC",
        r"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC",
        r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Tools\MSVC",
        r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Professional\VC\Tools\MSVC",
        r"C:\Program Files (x86)\Microsoft Visual Studio\2019\Community\VC\Tools\MSVC",
    ];
    for root in &roots {
        let root_path = std::path::Path::new(root);
        if let Ok(entries) = std::fs::read_dir(root_path) {
            let mut versions: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().into_string().ok())
                .collect();
            versions.sort_by(|a, b| b.cmp(a)); // newest first
            for ver in versions {
                let cl = format!(r"{}\{}\bin\Hostx64\x64", root, ver);
                if std::path::Path::new(&format!(r"{}\cl.exe", cl)).exists() {
                    return Some(cl);
                }
            }
        }
    }
    None
}

fn compile_cuda_sweep() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let kernel_src = "kernels/sweep.cu";
    let ptx_out = format!("{}/sweep.ptx", out_dir);

    // Allow overriding the compute arch at build time:
    //   CUDA_ARCH=compute_86 cargo build --features cuda   (for RTX 3080)
    let arch = std::env::var("CUDA_ARCH").unwrap_or_else(|_| "compute_75".to_string());

    let ccbin = find_msvc_bin();

    let nvcc_candidates = [
        "nvcc".to_string(),
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.2\bin\nvcc.exe".to_string(),
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v13.0\bin\nvcc.exe".to_string(),
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.6\bin\nvcc.exe".to_string(),
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.5\bin\nvcc.exe".to_string(),
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.4\bin\nvcc.exe".to_string(),
        r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.0\bin\nvcc.exe".to_string(),
        "/usr/local/cuda/bin/nvcc".to_string(),
        "/usr/bin/nvcc".to_string(),
    ];

    for nvcc in &nvcc_candidates {
        let mut cmd = std::process::Command::new(nvcc);
        cmd.args(&[
            kernel_src,
            "--ptx",
            &format!("-arch={}", arch),
            "-O3",
            "-o",
            &ptx_out,
        ]);
        if let Some(ref bin) = ccbin {
            cmd.args(&["-ccbin", bin.as_str()]);
        }
        let output = cmd.output();

        match output {
            Ok(o) if o.status.success() => {
                // Patch the PTX version header to 8.6 (max for CUDA 12.6 driver).
                if let Ok(src) = std::fs::read_to_string(&ptx_out) {
                    let patched = patch_ptx_version(&src, "8.6");
                    let _ = std::fs::write(&ptx_out, patched);
                }
                // Also assemble the patched PTX to CUBIN using ptxas (same toolkit).
                // The CUBIN is tried first at runtime as it avoids JIT overhead.
                let cubin_out = ptx_out.replace(".ptx", "_ptxas.cubin");
                let ptxas = nvcc.replace("nvcc.exe", "ptxas.exe").replace("nvcc", "ptxas");
                let _ = std::process::Command::new(&ptxas)
                    .args(&["-arch", &arch.replace("compute_", "sm_"), &ptx_out, "-o", &cubin_out])
                    .output();
                println!("cargo:rustc-env=SWEEP_PTX_PATH={}", ptx_out);
                println!("cargo:rerun-if-changed={}", kernel_src);
                eprintln!("build.rs: compiled {} → sweep.ptx + sweep_ptxas.cubin (arch={})", kernel_src, arch);
                return;
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("build.rs: nvcc at '{}' failed: {}", nvcc, stderr);
            }
            Err(_) => {
                // nvcc not found at this path, try next.
            }
        }
    }

    eprintln!(
        "build.rs: nvcc not found or failed — GPU will use fingerprint-only mode.\n\
         Install the CUDA Toolkit to enable full GPU embedding:\n\
         https://developer.nvidia.com/cuda-downloads"
    );
    println!("cargo:rerun-if-changed={}", kernel_src);
}

/// Patch PTX for older driver compatibility:
/// - Replace `.version X.Y` with `target_ver`
/// - Strip comment lines referencing the NVVM/nvcc toolchain version
///   (some driver versions scan comments to detect unsupported toolchains)
fn patch_ptx_version(src: &str, target_ver: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(".version ") {
            out.push_str(&format!(".version {}\n", target_ver));
        } else if trimmed.starts_with("//") {
            // Strip toolchain-identifying comments (NVVM version, nvcc version, build ID).
            // Keep only completely blank comment lines so the file stays readable.
            let comment = trimmed.trim_start_matches('/').trim();
            if comment.is_empty() {
                out.push_str(line);
                out.push('\n');
            }
            // else: drop the comment entirely
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
