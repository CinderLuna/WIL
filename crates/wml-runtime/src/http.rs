use crate::abi::{WmlHeader, WmlHttpRequest, WmlSlice};
use crate::arena::RequestArena;
use std::slice;

const MAX_HEADERS: usize = 64;

/// Zero-copy SIMD HTTP parser using `httparse`.
/// All headers, paths, and methods are extracted as slices pointing directly into `raw_buf`.
pub unsafe fn parse_http_request(
    arena: &RequestArena,
    raw_buf: &[u8],
) -> Result<WmlHttpRequest, &'static str> {
    let mut header_storage = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed_req = httparse::Request::new(&mut header_storage);

    let status = parsed_req
        .parse(raw_buf)
        .map_err(|_| "HTTP header parsing error")?;

    let body_offset = match status {
        httparse::Status::Complete(offset) => offset,
        httparse::Status::Partial => return Err("Incomplete HTTP request headers"),
    };

    let method_str = parsed_req.method.unwrap_or("GET").as_bytes();
    let path_str = parsed_req.path.unwrap_or("/").as_bytes();
    let body_bytes = &raw_buf[body_offset..];

    let num_headers = parsed_req.headers.len();
    let header_alloc_bytes = num_headers * std::mem::size_of::<WmlHeader>();
    let headers_ptr = if num_headers > 0 {
        let ptr = arena.alloc(header_alloc_bytes, std::mem::align_of::<WmlHeader>());
        if ptr.is_null() {
            return Err("Failed to allocate header storage in RequestArena");
        }
        let headers_dest = slice::from_raw_parts_mut(ptr as *mut WmlHeader, num_headers);
        for (i, h) in parsed_req.headers.iter().enumerate() {
            headers_dest[i] = WmlHeader {
                name: WmlSlice::from_slice(h.name.as_bytes()),
                value: WmlSlice::from_slice(h.value),
            };
        }
        ptr as *const WmlHeader
    } else {
        std::ptr::null()
    };

    Ok(WmlHttpRequest {
        method: WmlSlice::from_slice(method_str),
        path: WmlSlice::from_slice(path_str),
        body: WmlSlice::from_slice(body_bytes),
        headers: headers_ptr,
        num_headers,
    })
}

#[no_mangle]
pub unsafe extern "C" fn wml_http_parse_request(
    arena: *mut RequestArena,
    raw_buf: *const u8,
    raw_len: usize,
    out_req: *mut WmlHttpRequest,
) -> i32 {
    if arena.is_null() || raw_buf.is_null() || out_req.is_null() {
        return -1;
    }
    let arena_ref = &*arena;
    let buf = slice::from_raw_parts(raw_buf, raw_len);
    match parse_http_request(arena_ref, buf) {
        Ok(req) => {
            *out_req = req;
            0
        }
        Err(_) => -2,
    }
}
