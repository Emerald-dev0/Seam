use std::ffi::c_void;
use libc::{c_int, c_uint};

unsafe extern "C" {
    fn SHA256(d: *const u8, n: usize, md: *mut u8) -> *mut u8;
    fn EVP_sha256() -> *const c_void;
    fn HMAC(
        evp_md: *const c_void,
        key: *const c_void,
        key_len: c_int,
        d: *const u8,
        n: usize,
        md: *mut u8,
        md_len: *mut c_uint,
    ) -> *mut u8;
}

pub fn compute_sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    unsafe {
        SHA256(data.as_ptr(), data.len(), out.as_mut_ptr());
    }
    out
}

pub fn compute_hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut merged = Vec::new();
    for p in parts {
        merged.extend_from_slice(p);
    }
    let mut out = [0u8; 32];
    let mut out_len: c_uint = 0;
    unsafe {
        let md = EVP_sha256();
        HMAC(
            md,
            key.as_ptr() as *const c_void,
            key.len() as c_int,
            merged.as_ptr(),
            merged.len(),
            out.as_mut_ptr(),
            &mut out_len,
        );
    }
    out
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
