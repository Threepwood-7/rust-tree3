/// Attempt to lock a slice of data into physical RAM so the OS will not page it to swap.
///
/// Uses `VirtualLock` on Windows and `mlock` on Unix. Both are best-effort:
/// - Windows: requires the process working set to be large enough; the call to
///   `SetProcessWorkingSetSize` is made first to expand it, but it may still fail
///   without the "Lock pages in memory" privilege for very large regions.
/// - Unix: `mlock` may fail without `CAP_IPC_LOCK` for regions beyond `RLIMIT_MEMLOCK`.
///
/// Failure is non-fatal: a warning is printed and the program continues with
/// OS-managed (potentially paged) memory.
pub fn try_lock_in_ram<T>(slice: &[T], label: &str) {
    let bytes = std::mem::size_of_val(slice);
    if bytes == 0 {
        return;
    }
    let mb = bytes as f64 / 1_048_576.0;
    let ok = unsafe { lock_raw(slice.as_ptr() as *const u8, bytes) };
    if ok {
        eprintln!("  mlock: {label} ({mb:.1} MB) — pinned in physical RAM");
    } else {
        eprintln!("  mlock: {label} ({mb:.1} MB) — FAILED (OS may page this out)");
        #[cfg(windows)]
        eprintln!("         Tip: run as Administrator or grant 'Lock pages in memory' privilege");
        #[cfg(unix)]
        eprintln!("         Tip: raise RLIMIT_MEMLOCK or run as root");
    }
}

// ── Windows ──────────────────────────────────────────────────────────────────

#[cfg(windows)]
unsafe fn lock_raw(ptr: *const u8, len: usize) -> bool {
    use windows_sys::Win32::System::Memory::{SetProcessWorkingSetSizeEx, VirtualLock};
    use windows_sys::Win32::System::Memory::QUOTA_LIMITS_HARDWS_MIN_DISABLE;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let handle = GetCurrentProcess();
    // Expand the process working set so VirtualLock has room.
    // min = len + 128 MB overhead; max = 2× that. Failures are silently ignored.
    let min_ws = len.saturating_add(128 * 1024 * 1024);
    let max_ws = min_ws.saturating_mul(2);
    let _ = SetProcessWorkingSetSizeEx(handle, min_ws, max_ws, QUOTA_LIMITS_HARDWS_MIN_DISABLE);

    VirtualLock(ptr as *mut core::ffi::c_void, len) != 0
}

// ── Unix ─────────────────────────────────────────────────────────────────────

#[cfg(unix)]
unsafe fn lock_raw(ptr: *const u8, len: usize) -> bool {
    libc::mlock(ptr as *const libc::c_void, len) == 0
}

// ── Unsupported platform ─────────────────────────────────────────────────────

#[cfg(not(any(windows, unix)))]
unsafe fn lock_raw(_ptr: *const u8, _len: usize) -> bool {
    false
}
