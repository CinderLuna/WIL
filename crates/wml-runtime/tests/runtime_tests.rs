use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use wml_runtime::abi::*;
use wml_runtime::arena::RequestArena;
use wml_runtime::crypto::*;
use wml_runtime::http::parse_http_request;
use wml_runtime::reactor::HttpReactor;

#[test]
fn test_simd_http_request_parsing() {
    let arena = RequestArena::new();
    let raw_http = b"POST /auth/login HTTP/1.1\r\nHost: localhost:3000\r\nContent-Type: application/json\r\nContent-Length: 18\r\n\r\n{\"email\":\"a@b.com\"}";

    let req =
        unsafe { parse_http_request(&arena, raw_http).expect("Failed to parse HTTP request") };

    unsafe {
        assert_eq!(req.method.as_str(), "POST");
        assert_eq!(req.path.as_str(), "/auth/login");
        assert_eq!(req.body.as_str(), "{\"email\":\"a@b.com\"}");
        assert_eq!(req.num_headers, 3);
    }
}

#[test]
fn test_native_crypto_and_jwt() {
    let pass = b"secure_password_123";
    let hash = hash_password(pass);
    assert!(!hash.is_empty());

    assert!(verify_password(pass, hash.as_bytes()));
    assert!(!verify_password(b"wrong_password", hash.as_bytes()));

    let secret = b"my_secret_key_32_bytes_long_ok!";
    let token = sign_jwt("{\"sub\":123}", secret);
    assert!(!token.is_empty());
    assert!(verify_jwt(&token, secret));
    assert!(!verify_jwt("corrupt.token.value", secret));
}

unsafe extern "C" fn test_signup_handler(
    _arena: *mut RequestArena,
    _req: *const WmlHttpRequest,
    res: *mut WmlHttpResponse,
) -> i64 {
    const BODY: &[u8] = b"{\"token\":\"jwt_sample_123\"}";
    (*res).status = 200;
    (*res).body = WmlSlice::from_slice(BODY);
    0
}

#[test]
fn test_http_reactor_dispatch_and_response() {
    let mut reactor = HttpReactor::new();
    reactor.add_route("POST", "/auth/signup", test_signup_handler);

    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind test port");
    let addr = listener.local_addr().unwrap();

    let server_thread = thread::spawn(move || {
        let arena = RequestArena::new();
        if let Ok((stream, _)) = listener.accept() {
            let _ = reactor.handle_connection(stream, &arena);
        }
    });

    // Client connects and sends HTTP request
    let mut client = TcpStream::connect(addr).expect("Client connect failed");
    let req = b"POST /auth/signup HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n";
    client.write_all(req).unwrap();

    let mut response_buf = Vec::new();
    client.read_to_end(&mut response_buf).unwrap();
    let resp_str = String::from_utf8_lossy(&response_buf);

    assert!(resp_str.contains("HTTP/1.1 200 OK"));
    assert!(resp_str.contains("{\"token\":\"jwt_sample_123\"}"));

    server_thread.join().unwrap();
}
