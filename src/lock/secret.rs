//! The password being typed: a fixed-size heap buffer that is wiped whenever it is cleared, shrunk or dropped.
//!
//! The buffer never reallocates (so no stray copies are left behind by growth), is `mlock`ed best-effort so it
//! cannot be swapped out, and is overwritten with volatile writes (which the optimiser may not remove).

/// Largest password in bytes (PAM's `PAM_MAX_RESP_SIZE` is 512, including the terminator).
pub const MAX_BYTES: usize = 511;

pub struct Secret {
    buf: Box<[u8; MAX_BYTES + 1]>,
    len: usize,
    chars: usize,
}

/// Overwrites `b` with zeros in a way the compiler cannot elide.
pub fn wipe(b: &mut [u8]) {
    for byte in b.iter_mut() {
        // SAFETY: `byte` is a valid, aligned, exclusive reference.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

impl Secret {
    pub fn new() -> Secret {
        let buf = Box::new([0u8; MAX_BYTES + 1]);
        // SAFETY: the pointer and length describe the live heap allocation owned by `buf`; mlock only pins pages.
        unsafe { libc::mlock(buf.as_ptr().cast(), buf.len()) };
        Secret { buf, len: 0, chars: 0 }
    }

    /// Appends `c`; false (and nothing changes) when it would not fit.
    pub fn push(&mut self, c: char) -> bool {
        let n = c.len_utf8();
        if c == '\0' || self.len + n > MAX_BYTES {
            return false;
        }
        c.encode_utf8(&mut self.buf[self.len..self.len + n]);
        self.len += n;
        self.chars += 1;
        true
    }

    /// Removes the last character.
    pub fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        let mut start = self.len - 1;
        while start > 0 && self.buf[start] & 0xC0 == 0x80 {
            start -= 1;
        }
        wipe(&mut self.buf[start..self.len]);
        self.len = start;
        self.chars -= 1;
        true
    }

    pub fn clear(&mut self) {
        wipe(&mut self.buf[..]);
        self.len = 0;
        self.chars = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Characters typed (what the dots show).
    pub fn chars(&self) -> usize {
        self.chars
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl Default for Secret {
    fn default() -> Self {
        Secret::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        wipe(&mut self.buf[..]);
        // SAFETY: undoes the mlock of `new` on the same range.
        unsafe { libc::munlock(self.buf.as_ptr().cast(), self.buf.len()) };
    }
}

// Deliberately no Debug/Clone/Display: the contents must not be printable by accident.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({} chars)", self.chars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_and_utf8() {
        let mut s = Secret::new();
        for c in "aé€😀".chars() {
            assert!(s.push(c));
        }
        assert_eq!(s.chars(), 4);
        assert_eq!(s.as_bytes(), "aé€😀".as_bytes());
        assert!(s.pop());
        assert_eq!(s.as_bytes(), "aé€".as_bytes());
        assert_eq!(s.chars(), 3);
        s.clear();
        assert!(s.is_empty() && s.chars() == 0);
        assert!(!s.pop());
    }

    #[test]
    fn clear_and_pop_wipe_the_buffer() {
        let mut s = Secret::new();
        for c in "hunter2".chars() {
            s.push(c);
        }
        s.pop();
        assert_eq!(s.buf[6], 0, "popped byte is wiped");
        s.clear();
        assert!(s.buf.iter().all(|&b| b == 0), "cleared buffer is all zeros");
    }

    #[test]
    fn length_is_capped_and_nul_refused() {
        let mut s = Secret::new();
        assert!(!s.push('\0'));
        for _ in 0..MAX_BYTES {
            assert!(s.push('x'));
        }
        assert!(!s.push('y'));
        assert!(!s.push('€'));
        assert_eq!(s.chars(), MAX_BYTES);
        assert_eq!(format!("{s:?}"), format!("Secret({MAX_BYTES} chars)"));
    }
}
