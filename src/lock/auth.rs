//! Password checking. [`Authenticator`] is the seam the lock screen talks to; [`PamAuth`] is the real one (libpam
//! through hand-written FFI declarations, no extra crate), [`FakeAuth`] exists only for tests.

use std::ffi::{c_char, c_int, c_void, CString};

/// Why a check could not give a yes/no answer (PAM missing, misconfigured, or erroring).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthError(pub String);

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Checks a password for the user the lock was started by. `Ok(true)` only when the password is right.
/// Implementations run on a worker thread and may block (PAM sleeps a couple of seconds after a failure).
pub trait Authenticator: Send + Sync {
    fn authenticate(&self, password: &[u8]) -> Result<bool, AuthError>;
}

// ---------------------------------------------------------------- libpam FFI

const PAM_SUCCESS: c_int = 0;
const PAM_PERM_DENIED: c_int = 6;
const PAM_AUTH_ERR: c_int = 7;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_MAXTRIES: c_int = 11;
const PAM_CONV_ERR: c_int = 19;
const PAM_BUF_ERR: c_int = 5;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 1;
/// PAM refuses more than this many messages per conversation call.
const PAM_MAX_NUM_MSG: c_int = 32;

#[repr(C)]
pub struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
pub struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = unsafe extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<ConvFn>,
    appdata_ptr: *mut c_void,
}

#[repr(C)]
struct PamHandle {
    _private: [u8; 0],
}

#[link(name = "pam")]
extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, handle: *mut *mut PamHandle) -> c_int;
    fn pam_authenticate(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut PamHandle, status: c_int) -> c_int;
    fn pam_strerror(handle: *mut PamHandle, errnum: c_int) -> *const c_char;
}

/// What the conversation callback answers with.
struct ConvData {
    password: *const u8,
    password_len: usize,
    user: *const u8,
    user_len: usize,
}

/// `malloc`ed, NUL-terminated copy of `bytes` (PAM `free`s it, so it must come from the C allocator).
fn c_copy(bytes: &[u8]) -> *mut c_char {
    // SAFETY: malloc of len + 1 bytes; the copy stays within it and the last byte is the terminator.
    unsafe {
        let p = libc::malloc(bytes.len() + 1).cast::<u8>();
        if p.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
        *p.add(bytes.len()) = 0;
        p.cast()
    }
}

/// The PAM conversation: the password answers every hidden prompt, the user name every visible one; informational
/// messages need no answer. On any problem everything allocated so far is freed (zeroing the password copies).
///
/// # Safety
/// Called by libpam with `msg` pointing to `num_msg` valid message pointers and `resp` a valid out-pointer;
/// `appdata` is the [`ConvData`] that [`PamAuth`] passed to `pam_start` (valid for the whole call).
pub unsafe extern "C" fn conversation(num_msg: c_int, msg: *mut *const PamMessage, resp: *mut *mut PamResponse, appdata: *mut c_void) -> c_int {
    if num_msg <= 0 || num_msg > PAM_MAX_NUM_MSG || msg.is_null() || resp.is_null() || appdata.is_null() {
        return PAM_CONV_ERR;
    }
    let data = &*(appdata as *const ConvData);
    let n = num_msg as usize;
    let replies = libc::calloc(n, std::mem::size_of::<PamResponse>()).cast::<PamResponse>();
    if replies.is_null() {
        return PAM_BUF_ERR;
    }
    let free_all = |upto: usize| {
        for i in 0..upto {
            let r = &mut *replies.add(i);
            if !r.resp.is_null() {
                let len = libc::strlen(r.resp);
                super::secret::wipe(std::slice::from_raw_parts_mut(r.resp.cast::<u8>(), len));
                libc::free(r.resp.cast());
            }
        }
        libc::free(replies.cast());
    };
    for i in 0..n {
        let m = *msg.add(i);
        if m.is_null() {
            free_all(i);
            return PAM_CONV_ERR;
        }
        match (*m).msg_style {
            PAM_PROMPT_ECHO_OFF | PAM_PROMPT_ECHO_ON => {
                let (p, l) = if (*m).msg_style == PAM_PROMPT_ECHO_OFF { (data.password, data.password_len) } else { (data.user, data.user_len) };
                let copy = c_copy(std::slice::from_raw_parts(p, l));
                if copy.is_null() {
                    free_all(i);
                    return PAM_BUF_ERR;
                }
                (*replies.add(i)).resp = copy;
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO => {}
            _ => {
                free_all(i);
                return PAM_CONV_ERR;
            }
        }
    }
    *resp = replies;
    PAM_SUCCESS
}

/// The real thing: `pam_authenticate` for `user` under the PAM service `service` (`login` unless overridden).
pub struct PamAuth {
    service: CString,
    user: CString,
}

impl PamAuth {
    /// Checks that PAM can be started for `user` under `service` (so a broken setup is reported at start-up,
    /// before the screen is locked, and `rdwm-lock` can fall back to another locker).
    pub fn new(service: &str, user: &str) -> Result<PamAuth, AuthError> {
        let service = CString::new(service).map_err(|_| AuthError("PAM service name contains a NUL".into()))?;
        let user = CString::new(user).map_err(|_| AuthError("user name contains a NUL".into()))?;
        let me = PamAuth { service, user };
        let mut handle: *mut PamHandle = std::ptr::null_mut();
        let data = ConvData { password: [].as_ptr(), password_len: 0, user: me.user.as_ptr().cast(), user_len: me.user.as_bytes().len() };
        let conv = PamConv { conv: Some(conversation), appdata_ptr: (&data as *const ConvData).cast_mut().cast() };
        // SAFETY: valid NUL-terminated strings, a live conversation struct, and an out-pointer for the handle.
        let rc = unsafe { pam_start(me.service.as_ptr(), me.user.as_ptr(), &conv, &mut handle) };
        if rc != PAM_SUCCESS || handle.is_null() {
            return Err(AuthError(format!("pam_start failed ({rc})")));
        }
        // SAFETY: `handle` came from a successful pam_start.
        unsafe { pam_end(handle, PAM_SUCCESS) };
        Ok(me)
    }

    pub fn user(&self) -> &str {
        self.user.to_str().unwrap_or("")
    }
}

impl Authenticator for PamAuth {
    fn authenticate(&self, password: &[u8]) -> Result<bool, AuthError> {
        if password.contains(&0) {
            return Ok(false);
        }
        let data = ConvData { password: password.as_ptr(), password_len: password.len(), user: self.user.as_ptr().cast(), user_len: self.user.as_bytes().len() };
        let conv = PamConv { conv: Some(conversation), appdata_ptr: (&data as *const ConvData).cast_mut().cast() };
        let mut handle: *mut PamHandle = std::ptr::null_mut();
        // SAFETY: as in `new`; `data` and `conv` outlive the whole authentication (pam_end is called below).
        let rc = unsafe { pam_start(self.service.as_ptr(), self.user.as_ptr(), &conv, &mut handle) };
        if rc != PAM_SUCCESS || handle.is_null() {
            return Err(AuthError(format!("pam_start failed ({rc})")));
        }
        // SAFETY: valid handle; the conversation callback only reads `data`.
        let rc = unsafe { pam_authenticate(handle, PAM_DISALLOW_NULL_AUTHTOK) };
        let result = match rc {
            PAM_SUCCESS => Ok(true),
            PAM_AUTH_ERR | PAM_USER_UNKNOWN | PAM_MAXTRIES | PAM_PERM_DENIED | 9 /* PAM_CRED_INSUFFICIENT */ => Ok(false),
            other => {
                // SAFETY: pam_strerror returns a static string for any code (or NULL).
                let text = unsafe {
                    let p = pam_strerror(handle, other);
                    if p.is_null() {
                        String::new()
                    } else {
                        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                };
                Err(AuthError(format!("PAM error {other}: {text}")))
            }
        };
        // SAFETY: closes the handle opened above; the status passed is the last PAM result.
        unsafe { pam_end(handle, rc) };
        result
    }
}

// ---------------------------------------------------------------- test double

/// Accepts exactly one password. Only compiled for unit tests and for `--features test-auth` builds.
#[cfg(any(test, feature = "test-auth"))]
pub struct FakeAuth {
    password: Vec<u8>,
    /// Pause before answering (to exercise the "checking" state).
    pub delay: std::time::Duration,
}

#[cfg(any(test, feature = "test-auth"))]
impl FakeAuth {
    pub fn new(password: &str) -> FakeAuth {
        FakeAuth { password: password.as_bytes().to_vec(), delay: std::time::Duration::ZERO }
    }
}

#[cfg(any(test, feature = "test-auth"))]
impl Authenticator for FakeAuth {
    fn authenticate(&self, password: &[u8]) -> Result<bool, AuthError> {
        std::thread::sleep(self.delay);
        Ok(!self.password.is_empty() && password == self.password.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(style: c_int) -> PamMessage {
        PamMessage { msg_style: style, msg: c"Password: ".as_ptr() }
    }

    fn run_conv(styles: &[c_int], pw: &[u8]) -> (c_int, Vec<Option<Vec<u8>>>) {
        let msgs: Vec<PamMessage> = styles.iter().map(|s| msg(*s)).collect();
        let mut ptrs: Vec<*const PamMessage> = msgs.iter().map(|m| m as *const PamMessage).collect();
        let user = b"alice";
        let data = ConvData { password: pw.as_ptr(), password_len: pw.len(), user: user.as_ptr(), user_len: user.len() };
        let mut resp: *mut PamResponse = std::ptr::null_mut();
        // SAFETY: all pointers are valid for the call, as PAM would pass them.
        let rc = unsafe { conversation(styles.len() as c_int, ptrs.as_mut_ptr(), &mut resp, (&data as *const ConvData).cast_mut().cast()) };
        let mut out = Vec::new();
        if rc == PAM_SUCCESS {
            for i in 0..styles.len() {
                // SAFETY: `resp` holds `styles.len()` entries on success; strings are malloc'd NUL-terminated.
                unsafe {
                    let r = &*resp.add(i);
                    if r.resp.is_null() {
                        out.push(None);
                    } else {
                        out.push(Some(std::ffi::CStr::from_ptr(r.resp).to_bytes().to_vec()));
                        libc::free(r.resp.cast());
                    }
                }
            }
            // SAFETY: the array came from calloc in `conversation`.
            unsafe { libc::free(resp.cast()) };
        }
        (rc, out)
    }

    #[test]
    fn hidden_prompt_gets_the_password_visible_one_the_user() {
        let (rc, out) = run_conv(&[PAM_TEXT_INFO, PAM_PROMPT_ECHO_ON, PAM_PROMPT_ECHO_OFF, PAM_ERROR_MSG], b"s3cret");
        assert_eq!(rc, PAM_SUCCESS);
        assert_eq!(out, vec![None, Some(b"alice".to_vec()), Some(b"s3cret".to_vec()), None]);
    }

    #[test]
    fn unknown_style_and_bad_counts_fail_cleanly() {
        assert_eq!(run_conv(&[PAM_PROMPT_ECHO_OFF, 99], b"x").0, PAM_CONV_ERR);
        assert_eq!(run_conv(&[], b"x").0, PAM_CONV_ERR);
        let many = vec![PAM_TEXT_INFO; 40];
        assert_eq!(run_conv(&many, b"x").0, PAM_CONV_ERR);
    }

    #[test]
    fn empty_password_is_answered_with_an_empty_string() {
        let (rc, out) = run_conv(&[PAM_PROMPT_ECHO_OFF], b"");
        assert_eq!((rc, out), (PAM_SUCCESS, vec![Some(Vec::new())]));
    }

    #[test]
    fn fake_auth_accepts_only_its_password() {
        let a = FakeAuth::new("hunter2");
        assert_eq!(a.authenticate(b"hunter2"), Ok(true));
        assert_eq!(a.authenticate(b"hunter3"), Ok(false));
        assert_eq!(a.authenticate(b""), Ok(false));
        assert_eq!(FakeAuth::new("").authenticate(b""), Ok(false));
    }

    #[test]
    fn pam_starts_for_the_current_user() {
        // pam_start only loads the service's config; no authentication happens (and no password is involved).
        let user = std::env::var("USER").unwrap_or_else(|_| "nobody".into());
        assert!(PamAuth::new("login", &user).is_ok());
        assert!(PamAuth::new("lo\0gin", &user).is_err());
    }
}
