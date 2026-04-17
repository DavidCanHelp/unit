// End-to-end test for the HTTP bridge (`--features http`).
//
// Spins up the real `unit` binary with `--serve`, then hits each endpoint
// with a hand-written HTTP/1.1 request over a TcpStream. No test crates —
// the zero-dependency invariant extends to tests.

#![cfg(feature = "http")]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Reserve a port by binding 0, then dropping. There is a small window
/// between drop and the child's bind, but localhost is cheap to retry
/// and the test runs in isolation.
fn pick_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind 0");
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

struct UnitChild(Child);

impl Drop for UnitChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_unit(port: u16) -> UnitChild {
    let child = Command::new(env!("CARGO_BIN_EXE_unit"))
        .args([
            "--no-mesh",
            "--quiet",
            "--serve",
            &port.to_string(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn unit");
    UnitChild(child)
}

fn wait_for_listening(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("unit did not start listening on port {}", port);
}

fn http(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut s = TcpStream::connect(format!("127.0.0.1:{}", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let req = match body {
        Some(b) => format!(
            "{} {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            method,
            path,
            b.len(),
            b
        ),
        None => format!("{} {} HTTP/1.1\r\nHost: localhost\r\n\r\n", method, path),
    };
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::with_capacity(4096);
    s.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text
        .find("\r\n\r\n")
        .map(|i| text[i + 4..].to_string())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn bridge_endpoints() {
    let port = pick_port();
    let _child = spawn_unit(port);
    wait_for_listening(port);

    // /status — scalar fields present.
    let (s, body) = http(port, "GET", "/status", None);
    assert_eq!(s, 200, "status failed: {}", body);
    for key in [
        "\"id\"",
        "\"fitness\"",
        "\"energy\"",
        "\"peers\"",
        "\"words\"",
        "\"generation\"",
    ] {
        assert!(body.contains(key), "/status missing {}: {}", key, body);
    }

    // /eval — trivial arithmetic.
    let (s, body) = http(port, "POST", "/eval", Some(r#"{"source":"2 3 + ."}"#));
    assert_eq!(s, 200, "eval failed: {}", body);
    assert!(body.contains("\"output\""), "eval missing output: {}", body);
    assert!(body.contains('5'), "eval wrong result: {}", body);

    // /sexp — the classic (* 6 7).
    let (s, body) = http(port, "POST", "/sexp", Some(r#"{"expr":"(* 6 7)"}"#));
    assert_eq!(s, 200, "sexp failed: {}", body);
    assert!(body.contains("\"result\""), "sexp missing result: {}", body);
    assert!(body.contains("42"), "sexp wrong result: {}", body);

    // /words — contains at least one kernel primitive.
    let (s, body) = http(port, "GET", "/words", None);
    assert_eq!(s, 200);
    assert!(body.starts_with(r#"{"words":["#), "words shape: {}", body);
    assert!(body.contains("\"DUP\""), "DUP not in dictionary: {}", body);

    // Define a user word, then fetch its definition.
    let (s, _) = http(port, "POST", "/eval", Some(r#"{"source":": SQR DUP * ;"}"#));
    assert_eq!(s, 200);
    let (s, body) = http(port, "GET", "/word/SQR", None);
    assert_eq!(s, 200, "word/SQR failed: {}", body);
    assert!(body.contains("\"name\":\"SQR\""));
    assert!(body.contains("DUP"));

    // Unknown word: 404 with JSON error.
    let (s, body) = http(port, "GET", "/word/NOPENOPENOPE", None);
    assert_eq!(s, 404);
    assert!(body.contains("\"error\""));

    // /mesh/peers — mesh disabled, returns empty array (not an error).
    let (s, body) = http(port, "GET", "/mesh/peers", None);
    assert_eq!(s, 200);
    assert_eq!(body.trim(), r#"{"peers":[]}"#);

    // /mesh/broadcast — mesh disabled, returns 503.
    let (s, body) = http(
        port,
        "POST",
        "/mesh/broadcast",
        Some(r#"{"expr":"(hi)"}"#),
    );
    assert_eq!(s, 503);
    assert!(body.contains("\"error\""));

    // Unknown route: 404.
    let (s, _) = http(port, "GET", "/does-not-exist", None);
    assert_eq!(s, 404);

    // Known path, wrong method: 405.
    let (s, _) = http(port, "GET", "/eval", None);
    assert_eq!(s, 405);

    // Missing required field: 400.
    let (s, body) = http(port, "POST", "/eval", Some(r#"{"nope":"x"}"#));
    assert_eq!(s, 400);
    assert!(body.contains("\"error\""));
}

#[test]
fn rejects_oversized_body() {
    let port = pick_port();
    let _child = spawn_unit(port);
    wait_for_listening(port);

    // Declare a huge Content-Length without sending the body. The server
    // should reject on the header value alone — that avoids a TCP RST race
    // where macOS resets the connection because of unread data at close.
    let mut s = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req =
        "POST /eval HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100000\r\n\r\n";
    s.write_all(req.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.starts_with("HTTP/1.1 413"),
        "expected 413, got: {}",
        text
    );
}
