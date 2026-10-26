//! Small glibc allocator tweaks that keep resident memory low after bursts of image decoding on
//! worker threads (each thread would otherwise keep its own arena). No-ops on other libcs.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod sys {
    extern "C" {
        pub fn mallopt(param: i32, value: i32) -> i32;
        pub fn malloc_trim(pad: usize) -> i32;
    }
    /// `M_ARENA_MAX` from `<malloc.h>`.
    pub const M_ARENA_MAX: i32 = -8;
    /// `M_MMAP_THRESHOLD`: allocations at least this big get their own mapping.
    pub const M_MMAP_THRESHOLD: i32 = -3;
}

/// Share one malloc arena between all threads, and pin the mmap threshold so big blocks (a parsed
/// font face, an image) always get their own mapping that is returned on free. glibc otherwise raises
/// the threshold to the size of the largest block freed so far and later parks such blocks on the
/// heap, where they fragment it and stay resident. Call once, early in `main`.
pub fn limit_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `mallopt` only changes allocator tuning and is safe to call at any time.
    unsafe {
        sys::mallopt(sys::M_ARENA_MAX, 1);
        sys::mallopt(sys::M_MMAP_THRESHOLD, 256 * 1024);
    }
}

/// Returns freed heap pages to the OS.
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` is always safe to call; it only releases free memory.
    unsafe {
        sys::malloc_trim(0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn calls_are_harmless() {
        super::limit_arenas();
        let v = vec![1u8; 4 << 20];
        drop(v);
        super::trim();
    }
}
