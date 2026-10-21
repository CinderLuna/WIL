use crate::abi::{WmlHttpRequest, WmlHttpResponse, WmlSlice};
use crate::arena::RequestArena;
use crate::http::parse_http_request;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub type RouteHandlerFn =
    unsafe extern "C" fn(*mut RequestArena, *const WmlHttpRequest, *mut WmlHttpResponse) -> i64;

#[derive(Clone)]
pub struct HttpRoute {
    pub method: String,
    pub path: String,
    pub handler: RouteHandlerFn,
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

pub struct HttpReactor {
    routes: Arc<HashMap<(String, String), RouteHandlerFn>>,
    running: Arc<AtomicBool>,
}

impl Default for HttpReactor {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpReactor {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(HashMap::new()),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn add_route(&mut self, method: &str, path: &str, handler: RouteHandlerFn) {
        if let Some(map) = Arc::get_mut(&mut self.routes) {
            map.insert((method.to_uppercase(), path.to_string()), handler);
        } else {
            let mut new_map = (*self.routes).clone();
            new_map.insert((method.to_uppercase(), path.to_string()), handler);
            self.routes = Arc::new(new_map);
        }
    }

    pub fn handle_connection(
        &self,
        mut stream: TcpStream,
        arena: &RequestArena,
    ) -> std::io::Result<()> {
        let mut buffer = [0u8; 64 * 1024];
        let mut total_read = 0;

        loop {
            let bytes_read = stream.read(&mut buffer[total_read..])?;
            if bytes_read == 0 {
                break;
            }
            total_read += bytes_read;

            let raw_req = &buffer[..total_read];
            let mut header_storage = [httparse::EMPTY_HEADER; 64];
            let mut parsed = httparse::Request::new(&mut header_storage);
            match parsed.parse(raw_req) {
                Ok(httparse::Status::Complete(_)) => break,
                Ok(httparse::Status::Partial) if total_read < buffer.len() => continue,
                _ => break,
            }
        }

        if total_read == 0 {
            return Ok(());
        }

        let raw_req = &buffer[..total_read];
        let mut response = WmlHttpResponse::new();

        unsafe {
            match parse_http_request(arena, raw_req) {
                Ok(req) => {
                    let method = req.method.as_str();
                    let path = req.path.as_str();

                    if let Some(handler) = self.routes.get(&(method.to_string(), path.to_string()))
                    {
                        let arena_ptr = arena as *const RequestArena as *mut RequestArena;
                        handler(arena_ptr, &req, &mut response);
                    } else {
                        // 404 Not Found
                        response.status = 404;
                        const NOT_FOUND_BODY: &[u8] = b"{\"error\":\"Not Found\"}";
                        response.body = WmlSlice::from_slice(NOT_FOUND_BODY);
                    }
                }
                Err(err) => {
                    // 400 Bad Request
                    response.status = 400;
                    response.body = WmlSlice::from_slice(err.as_bytes());
                }
            }

            // Write HTTP/1.1 response
            let body_bytes = response.body.as_bytes();
            let ct_str = response.content_type.as_str();
            let reason = status_text(response.status);
            let header_str = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.status, reason, ct_str, body_bytes.len()
            );

            stream.write_all(header_str.as_bytes())?;
            if !body_bytes.is_empty() {
                stream.write_all(body_bytes)?;
            }
            stream.flush()?;

            // Zero-cost teardown: reset request arena back to base pointer
            arena.reset();
        }

        Ok(())
    }

    /// Serves incoming requests on the given listener.
    pub fn serve(&self, listener: TcpListener) -> std::io::Result<()> {
        self.running.store(true, Ordering::SeqCst);
        let arena = RequestArena::new();

        for stream in listener.incoming() {
            if !self.running.load(Ordering::SeqCst) {
                break;
            }
            match stream {
                Ok(s) => {
                    let _ = self.handle_connection(s, &arena);
                }
                Err(e) => {
                    eprintln!("Socket accept error: {:?}", e);
                }
            }
        }
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
