//! Small glibc allocator tweaks that keep resident memory low after bursts of image decoding and font parsing
//! on worker threads. No-ops on other libcs.

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

/// Two malloc arenas: the main one and one shared by every worker thread (not one per thread). Work that makes
/// a lot of short-lived small allocations (parsing a font face: 13 MB of glyph outlines; decoding an image) runs
/// on a worker, so it cannot interleave with long-lived data of the main heap and pin its pages. Also pins the
/// mmap threshold so big blocks always get their own mapping that is returned on free: glibc otherwise raises
/// the threshold to the size of the largest block freed so far and later parks such blocks on the heap, where
/// they fragment it and stay resident. Call once, early in `main`.
pub fn limit_arenas() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `mallopt` only changes allocator tuning and is safe to call at any time.
    unsafe {
        sys::mallopt(sys::M_ARENA_MAX, 2);
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
