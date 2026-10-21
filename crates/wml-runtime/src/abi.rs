use crate::arena::RequestArena;
use crate::crypto::{hash_password, sign_jwt, verify_jwt, verify_password};
use std::slice;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WmlSlice {
    pub ptr: *const u8,
    pub len: usize,
}

impl WmlSlice {
    pub fn empty() -> Self {
        Self {
            ptr: std::ptr::null(),
            len: 0,
        }
    }

    pub fn from_slice(s: &[u8]) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    pub unsafe fn as_bytes(&self) -> &'static [u8] {
        if self.ptr.is_null() || self.len == 0 {
            &[]
        } else {
            slice::from_raw_parts(self.ptr, self.len)
        }
    }

    pub unsafe fn as_str(&self) -> &'static str {
        std::str::from_utf8_unchecked(self.as_bytes())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WmlHeader {
    pub name: WmlSlice,
    pub value: WmlSlice,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct WmlHttpRequest {
    pub method: WmlSlice,
    pub path: WmlSlice,
    pub body: WmlSlice,
    pub headers: *const WmlHeader,
    pub num_headers: usize,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct WmlHttpResponse {
    pub status: u16,
    pub content_type: WmlSlice,
    pub body: WmlSlice,
}

impl Default for WmlHttpResponse {
    fn default() -> Self {
        Self::new()
    }
}

impl WmlHttpResponse {
    pub fn new() -> Self {
        const DEFAULT_CT: &[u8] = b"application/json";
        Self {
            status: 200,
            content_type: WmlSlice::from_slice(DEFAULT_CT),
            body: WmlSlice::empty(),
        }
    }
}

// ----------------------------------------------------------------------------
// Pure C-ABI Exports (Links identically with Cranelift JIT & LLVM AOT)
// ----------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn wml_arena_alloc(
    arena: *mut RequestArena,
    size: usize,
    align: usize,
) -> *mut u8 {
    if arena.is_null() {
        return std::ptr::null_mut();
    }
    (*arena).alloc(size, align)
}

#[no_mangle]
pub unsafe extern "C" fn wml_arena_reset(arena: *mut RequestArena) {
    if !arena.is_null() {
        (*arena).reset();
    }
}

#[no_mangle]
pub unsafe extern "C" fn wml_crypto_hash(
    arena: *mut RequestArena,
    pass_ptr: *const u8,
    pass_len: usize,
    out_hash: *mut WmlSlice,
) -> i32 {
    if pass_ptr.is_null() || out_hash.is_null() || arena.is_null() {
        return -1;
    }
    let pass = slice::from_raw_parts(pass_ptr, pass_len);
    let hash = hash_password(pass);

    let arena_ref = &*arena;
    let dest_ptr = arena_ref.alloc(hash.len(), 1);
    if dest_ptr.is_null() {
        return -2;
    }
    std::ptr::copy_nonoverlapping(hash.as_ptr(), dest_ptr, hash.len());

    *out_hash = WmlSlice {
        ptr: dest_ptr,
        len: hash.len(),
    };
    0
}

#[no_mangle]
pub unsafe extern "C" fn wml_crypto_verify(
    pass_ptr: *const u8,
    pass_len: usize,
    hash_ptr: *const u8,
    hash_len: usize,
) -> bool {
    if pass_ptr.is_null() || hash_ptr.is_null() {
        return false;
    }
    let pass = slice::from_raw_parts(pass_ptr, pass_len);
    let hash = slice::from_raw_parts(hash_ptr, hash_len);
    verify_password(pass, hash)
}

#[no_mangle]
pub unsafe extern "C" fn wml_jwt_sign(
    arena: *mut RequestArena,
    payload_ptr: *const u8,
    payload_len: usize,
    out_token: *mut WmlSlice,
) -> i32 {
    if payload_ptr.is_null() || out_token.is_null() || arena.is_null() {
        return -1;
    }
    let payload =
        std::str::from_utf8(slice::from_raw_parts(payload_ptr, payload_len)).unwrap_or("{}");
    const DEFAULT_SECRET: &[u8] = b"wml_super_secret_signing_key_32_bytes!";
    let token = sign_jwt(payload, DEFAULT_SECRET);

    let arena_ref = &*arena;
    let dest_ptr = arena_ref.alloc(token.len(), 1);
    if dest_ptr.is_null() {
        return -2;
    }
    std::ptr::copy_nonoverlapping(token.as_ptr(), dest_ptr, token.len());

    *out_token = WmlSlice {
        ptr: dest_ptr,
        len: token.len(),
    };
    0
}

#[no_mangle]
pub unsafe extern "C" fn wml_jwt_verify(token_ptr: *const u8, token_len: usize) -> bool {
    if token_ptr.is_null() {
        return false;
    }
    let token = std::str::from_utf8(slice::from_raw_parts(token_ptr, token_len)).unwrap_or("");
    const DEFAULT_SECRET: &[u8] = b"wml_super_secret_signing_key_32_bytes!";
    verify_jwt(token, DEFAULT_SECRET)
}

#[no_mangle]
pub unsafe extern "C" fn wml_res_set_body(
    res: *mut WmlHttpResponse,
    body_ptr: *const u8,
    body_len: usize,
) {
    if !res.is_null() {
        (*res).body = WmlSlice {
            ptr: body_ptr,
            len: body_len,
        };
    }
}

#[no_mangle]
pub unsafe extern "C" fn wml_res_set_status(res: *mut WmlHttpResponse, status: u16) {
    if !res.is_null() {
        (*res).status = status;
    }
}
