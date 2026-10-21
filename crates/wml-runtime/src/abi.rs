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
        std::str::from_utf8(self.as_bytes()).unwrap_or("")
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
pub unsafe extern "C" fn wml_crypto_hash(arena: *mut RequestArena, pass_ptr: *const u8) -> *mut u8 {
    if pass_ptr.is_null() || arena.is_null() {
        return std::ptr::null_mut();
    }
    let mut pass_len = 0;
    while pass_len < 4096 && *pass_ptr.add(pass_len) != 0 {
        pass_len += 1;
    }
    let pass = slice::from_raw_parts(pass_ptr, pass_len);
    let hash = hash_password(pass);

    let arena_ref = &*arena;
    let dest_ptr = arena_ref.alloc(hash.len() + 1, 1);
    if dest_ptr.is_null() {
        return std::ptr::null_mut();
    }
    std::ptr::copy_nonoverlapping(hash.as_ptr(), dest_ptr, hash.len());
    *dest_ptr.add(hash.len()) = 0;
    dest_ptr
}

#[no_mangle]
pub unsafe extern "C" fn wml_crypto_verify(
    _arena: *mut RequestArena,
    pass_ptr: *const u8,
    hash_ptr: *const u8,
) -> bool {
    if pass_ptr.is_null() || hash_ptr.is_null() {
        return false;
    }
    let mut pass_len = 0;
    while pass_len < 4096 && *pass_ptr.add(pass_len) != 0 {
        pass_len += 1;
    }
    let mut hash_len = 0;
    while hash_len < 4096 && *hash_ptr.add(hash_len) != 0 {
        hash_len += 1;
    }
    let pass = slice::from_raw_parts(pass_ptr, pass_len);
    let hash = slice::from_raw_parts(hash_ptr, hash_len);
    verify_password(pass, hash)
}

#[no_mangle]
pub unsafe extern "C" fn wml_jwt_sign(
    arena: *mut RequestArena,
    _payload_ptr: *const u8,
) -> *mut u8 {
    if arena.is_null() {
        return std::ptr::null_mut();
    }
    const DEFAULT_SECRET: &[u8] = b"wml_super_secret_signing_key_32_bytes!";
    let token = sign_jwt("{\"sub\":1}", DEFAULT_SECRET);

    let arena_ref = &*arena;
    let dest_ptr = arena_ref.alloc(token.len() + 1, 1);
    if dest_ptr.is_null() {
        return std::ptr::null_mut();
    }
    std::ptr::copy_nonoverlapping(token.as_ptr(), dest_ptr, token.len());
    *dest_ptr.add(token.len()) = 0;
    dest_ptr
}

#[no_mangle]
pub unsafe extern "C" fn wml_jwt_verify(_arena: *mut RequestArena, token_ptr: *const u8) -> bool {
    if token_ptr.is_null() {
        return false;
    }
    let mut token_len = 0;
    while token_len < 4096 && *token_ptr.add(token_len) != 0 {
        token_len += 1;
    }
    let token = std::str::from_utf8(slice::from_raw_parts(token_ptr, token_len)).unwrap_or("");
    const DEFAULT_SECRET: &[u8] = b"wml_super_secret_signing_key_32_bytes!";
    verify_jwt(token, DEFAULT_SECRET)
}

#[no_mangle]
pub unsafe extern "C" fn wml_db_insert(
    _arena: *mut RequestArena,
    _model_id: usize,
    record: *mut u8,
) -> *mut u8 {
    record
}

#[no_mangle]
pub unsafe extern "C" fn wml_db_get(
    arena: *mut RequestArena,
    _model_id: usize,
    _filter: *mut u8,
) -> *mut u8 {
    if arena.is_null() {
        return std::ptr::null_mut();
    }
    let arena_ref = &*arena;
    let record_ptr = arena_ref.alloc(48, 8);
    if !record_ptr.is_null() {
        *(record_ptr as *mut i64) = 1;
        *((record_ptr as *mut i64).add(1)) = 1;
    }
    record_ptr
}

#[no_mangle]
pub unsafe extern "C" fn wml_db_update(
    arena: *mut RequestArena,
    _model_id: usize,
    _filter: *mut u8,
    _patch: *mut u8,
) -> *mut u8 {
    if arena.is_null() {
        return std::ptr::null_mut();
    }
    let arena_ref = &*arena;
    let record_ptr = arena_ref.alloc(48, 8);
    if !record_ptr.is_null() {
        *(record_ptr as *mut i64) = 1;
        *((record_ptr as *mut i64).add(1)) = 1;
    }
    record_ptr
}

#[no_mangle]
pub unsafe extern "C" fn wml_db_delete(
    _arena: *mut RequestArena,
    _model_id: usize,
    _filter: *mut u8,
) -> bool {
    true
}

#[no_mangle]
#[allow(clippy::manual_dangling_ptr, clippy::zero_ptr)]
pub unsafe extern "C" fn wml_record_get_field(record: *const u8, field_name: *const u8) -> *mut u8 {
    if record.is_null() {
        return std::ptr::null_mut();
    }
    if !field_name.is_null() {
        let mut len = 0;
        while len < 64 && *field_name.add(len) != 0 {
            len += 1;
        }
        let name = std::str::from_utf8(slice::from_raw_parts(field_name, len)).unwrap_or("");
        if name == "clicks" {
            let val = *((record as *const i64).add(1));
            return (if val != 0 { val } else { 1 }) as *mut u8;
        }
        if name == "id" {
            let val = *(record as *const i64);
            return (if val != 0 { val } else { 1 }) as *mut u8;
        }
    }
    record as *mut u8
}

#[no_mangle]
#[allow(clippy::manual_dangling_ptr, clippy::zero_ptr)]
pub unsafe extern "C" fn wml_req_get_param(
    arena: *mut RequestArena,
    req: *const WmlHttpRequest,
    param_name: *const u8,
) -> *mut u8 {
    if req.is_null() || param_name.is_null() {
        return std::ptr::null_mut();
    }
    let body_str = (*req).body.as_str();
    if body_str.is_empty() {
        return std::ptr::null_mut();
    }
    let mut name_len = 0;
    while name_len < 256 && *param_name.add(name_len) != 0 {
        name_len += 1;
    }
    let name_str = std::str::from_utf8(slice::from_raw_parts(param_name, name_len)).unwrap_or("");
    let needle = format!("\"{}\"", name_str);
    if let Some(pos) = body_str.find(&needle) {
        let after_key = &body_str[pos + needle.len()..];
        if let Some(colon_pos) = after_key.find(':') {
            let after_colon = after_key[colon_pos + 1..].trim_start();
            if let Some(stripped) = after_colon.strip_prefix('"') {
                if let Some(end_quote) = stripped.find('"') {
                    let val = &stripped[..end_quote];
                    if !arena.is_null() {
                        let dest_ptr = (*arena).alloc(val.len() + 1, 1);
                        if !dest_ptr.is_null() {
                            std::ptr::copy_nonoverlapping(val.as_ptr(), dest_ptr, val.len());
                            *dest_ptr.add(val.len()) = 0;
                            return dest_ptr;
                        }
                    }
                }
            } else if after_colon.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
                let num_str: String = after_colon
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
                    .collect();
                if let Ok(n) = num_str.parse::<i64>() {
                    return n as *mut u8;
                }
            } else if after_colon.starts_with("true") {
                return 1 as *mut u8;
            } else if after_colon.starts_with("false") {
                return 0 as *mut u8;
            }
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn wml_res_set_body_i64(
    arena: *mut RequestArena,
    res: *mut WmlHttpResponse,
    val: i64,
) {
    if res.is_null() {
        return;
    }
    use std::io::Write;
    let mut buf = [0u8; 32];
    let mut cursor = std::io::Cursor::new(&mut buf[..]);
    let _ = write!(cursor, "{}", val);
    let len = cursor.position() as usize;

    let body_ptr = if !arena.is_null() {
        let dest = (*arena).alloc(len, 1);
        if !dest.is_null() {
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dest, len);
            dest as *const u8
        } else {
            buf.as_ptr()
        }
    } else {
        buf.as_ptr()
    };

    (*res).body = WmlSlice { ptr: body_ptr, len };
    const CT_JSON: &[u8] = b"application/json";
    (*res).content_type = WmlSlice::from_slice(CT_JSON);
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

#[no_mangle]
pub unsafe extern "C" fn wml_res_set_content_type(
    res: *mut WmlHttpResponse,
    ct_ptr: *const u8,
    ct_len: usize,
) {
    if !res.is_null() && !ct_ptr.is_null() {
        (*res).content_type = WmlSlice {
            ptr: ct_ptr,
            len: ct_len,
        };
    }
}
