//! HTTP bridge — exposes the VM and mesh over localhost.
//!
//! A minimal, hand-rolled HTTP/1.1 server. Zero dependencies. Binds to
//! 127.0.0.1 only. One std::thread per connection; `Connection: close`
//! after every response. Requests over 64 KiB are rejected with 413.
//! Socket reads time out after 5 seconds.
//!
//! The bridge is thin: endpoints translate to existing VM and mesh APIs,
//! they do not add behavior. Keep it that way.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::mesh;
use crate::sexp;
use crate::snapshot;
use crate::vm::VM;

pub const DEFAULT_PORT: u16 = 9898;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Start serving HTTP requests on 127.0.0.1:port. Blocks forever.
///
/// Prints the bound address (resolves port 0) and then accepts forever.
/// Each connection is handled on its own thread; the VM is shared via
/// Arc<Mutex<VM>>. There is no keep-alive — connections close after one
/// request-response cycle.
pub fn serve(vm: VM, port: u16) -> ! {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("http: bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    let bound = listener.local_addr().unwrap_or(addr);
    eprintln!("http: listening on http://{}", bound);

    let vm = Arc::new(Mutex::new(vm));

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let vm = Arc::clone(&vm);
                thread::spawn(move || {
                    let _ = handle_connection(stream, vm);
                });
            }
            Err(e) => {
                eprintln!("http: accept: {}", e);
            }
        }
    }
    std::process::exit(0);
}

// ---------------------------------------------------------------------------
// Request parsing
// ---------------------------------------------------------------------------

struct Request {
    method: String,
    path: String,
    body: String,
}

struct HttpError {
    status: u16,
    msg: String,
}

fn handle_connection(mut stream: TcpStream, vm: Arc<Mutex<VM>>) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));

    let (status, body) = match read_request(&mut stream) {
        Ok(req) => route(&req, vm),
        Err(e) => (e.status, error_json(&e.msg)),
    };
    write_response(&mut stream, status, "application/json", &body)
}

fn read_request(stream: &mut TcpStream) -> Result<Request, HttpError> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut tmp = [0u8; 1024];
    let header_end = loop {
        let n = stream.read(&mut tmp).map_err(|e| HttpError {
            status: 400,
            msg: format!("read: {}", e),
        })?;
        if n == 0 {
            return Err(HttpError {
                status: 400,
                msg: "empty request".into(),
            });
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > MAX_REQUEST_BYTES {
            return Err(HttpError {
                status: 413,
                msg: "request too large".into(),
            });
        }
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };

    let header_str = std::str::from_utf8(&buf[..header_end]).map_err(|_| HttpError {
        status: 400,
        msg: "non-utf8 headers".into(),
    })?;
    let mut lines = header_str.split("\r\n");
    let start_line = lines.next().ok_or_else(|| HttpError {
        status: 400,
        msg: "missing start line".into(),
    })?;
    let mut parts = start_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| HttpError {
            status: 400,
            msg: "missing method".into(),
        })?
        .to_string();
    let raw_path = parts.next().ok_or_else(|| HttpError {
        status: 400,
        msg: "missing path".into(),
    })?;
    let path = raw_path.split('?').next().unwrap_or("/").to_string();

    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().map_err(|_| HttpError {
                    status: 400,
                    msg: "bad content-length".into(),
                })?;
            }
        }
    }

    if content_length > MAX_REQUEST_BYTES {
        return Err(HttpError {
            status: 413,
            msg: "body too large".into(),
        });
    }

    let body_start = header_end + 4;
    let mut body_bytes: Vec<u8> = if body_start < buf.len() {
        buf[body_start..].to_vec()
    } else {
        Vec::new()
    };
    while body_bytes.len() < content_length {
        let need = content_length - body_bytes.len();
        let cap = tmp.len();
        let n = stream
            .read(&mut tmp[..need.min(cap)])
            .map_err(|e| HttpError {
                status: 400,
                msg: format!("read body: {}", e),
            })?;
        if n == 0 {
            return Err(HttpError {
                status: 400,
                msg: "short body".into(),
            });
        }
        body_bytes.extend_from_slice(&tmp[..n]);
        if body_bytes.len() + body_start > MAX_REQUEST_BYTES {
            return Err(HttpError {
                status: 413,
                msg: "body too large".into(),
            });
        }
    }
    body_bytes.truncate(content_length);
    let body = String::from_utf8(body_bytes).map_err(|_| HttpError {
        status: 400,
        msg: "non-utf8 body".into(),
    })?;

    Ok(Request {
        method,
        path,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    (0..=buf.len() - 4).find(|&i| &buf[i..i + 4] == b"\r\n\r\n")
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        reason,
        content_type,
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

fn route(req: &Request, vm: Arc<Mutex<VM>>) -> (u16, String) {
    let path = req.path.as_str();
    match (req.method.as_str(), path) {
        ("POST", "/eval") => endpoint_eval(&req.body, vm),
        ("POST", "/sexp") => endpoint_sexp(&req.body, vm),
        ("GET", "/status") => endpoint_status(vm),
        ("GET", "/words") => endpoint_words(vm),
        ("GET", "/mesh/peers") => endpoint_mesh_peers(vm),
        ("POST", "/mesh/broadcast") => endpoint_mesh_broadcast(&req.body, vm),
        ("GET", p) if p.starts_with("/word/") => endpoint_word(&p[6..], vm),
        (_, p) if is_known_path(p) => (405, error_json("method not allowed")),
        _ => (404, error_json("not found")),
    }
}

fn is_known_path(p: &str) -> bool {
    matches!(
        p,
        "/eval" | "/sexp" | "/status" | "/words" | "/mesh/peers" | "/mesh/broadcast"
    ) || p.starts_with("/word/")
}

// ---------------------------------------------------------------------------
// Endpoints — translation, not logic
// ---------------------------------------------------------------------------

fn endpoint_eval(body: &str, vm: Arc<Mutex<VM>>) -> (u16, String) {
    let source = match json_get_string(body, "source") {
        Some(s) => s,
        None => return (400, error_json("missing or invalid \"source\" field")),
    };
    let mut vm = vm.lock().unwrap();
    let output = vm.eval(&source);
    (200, json_object_str("output", &output))
}

fn endpoint_sexp(body: &str, vm: Arc<Mutex<VM>>) -> (u16, String) {
    let expr = match json_get_string(body, "expr") {
        Some(s) => s,
        None => return (400, error_json("missing or invalid \"expr\" field")),
    };
    let parsed = match sexp::parse(&expr) {
        Ok(p) => p,
        Err(e) => return (400, error_json(&format!("sexp parse: {}", e.0))),
    };
    let forth = sexp::to_forth(&parsed);
    let mut vm = vm.lock().unwrap();
    let before = vm.stack.len();
    let output = vm.eval(&forth);
    // If the expression produced printed output, that IS the result.
    // Otherwise fall back to peeking the top of the stack — S-expressions
    // like `(* 6 7)` leave their value there. Don't pop: matches SEXP".
    let trimmed = output.trim_end();
    let result = if !trimmed.is_empty() {
        trimmed.to_string()
    } else if vm.stack.len() > before {
        vm.stack
            .last()
            .map(|c| c.to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    (200, json_object_str("result", &result))
}

fn endpoint_status(vm: Arc<Mutex<VM>>) -> (u16, String) {
    let vm = vm.lock().unwrap();
    let id = vm
        .node_id_cache
        .map(|b| mesh::id_to_hex(&b))
        .unwrap_or_default();
    let peers = vm.mesh.as_ref().map(|m| m.peer_count()).unwrap_or(0);
    let mut out = String::with_capacity(256);
    out.push('{');
    out.push_str(&format!(
        "\"id\":\"{}\",",
        snapshot::escape_json_string(&id)
    ));
    out.push_str(&format!("\"fitness\":{},", vm.fitness.score));
    out.push_str(&format!("\"energy\":{},", vm.energy.energy));
    out.push_str(&format!("\"peers\":{},", peers));
    out.push_str(&format!("\"words\":{},", vm.dictionary.len()));
    out.push_str(&format!("\"generation\":{}", vm.spawn_state.generation));
    out.push('}');
    (200, out)
}

fn endpoint_words(vm: Arc<Mutex<VM>>) -> (u16, String) {
    let vm = vm.lock().unwrap();
    let mut out = String::with_capacity(1024);
    out.push_str("{\"words\":[");
    for (i, entry) in vm.dictionary.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(&snapshot::escape_json_string(&entry.name));
        out.push('"');
    }
    out.push_str("]}");
    (200, out)
}

fn endpoint_word(name_part: &str, vm: Arc<Mutex<VM>>) -> (u16, String) {
    let name = match percent_decode(name_part) {
        Some(n) => n,
        None => return (400, error_json("bad percent-encoding")),
    };
    let vm = vm.lock().unwrap();
    let entry = match vm.dictionary.iter().find(|e| e.name == name) {
        Some(e) => e,
        None => return (404, error_json("word not found")),
    };
    let def = snapshot::decompile_word(entry, &vm.dictionary, &vm.primitive_names);
    let mut out = String::with_capacity(name.len() + def.len() + 64);
    out.push('{');
    out.push_str(&format!(
        "\"name\":\"{}\",",
        snapshot::escape_json_string(&name)
    ));
    out.push_str(&format!(
        "\"definition\":\"{}\"",
        snapshot::escape_json_string(&def)
    ));
    out.push('}');
    (200, out)
}

fn endpoint_mesh_peers(vm: Arc<Mutex<VM>>) -> (u16, String) {
    let vm = vm.lock().unwrap();
    let mut out = String::with_capacity(256);
    out.push_str("{\"peers\":[");
    if let Some(ref m) = vm.mesh {
        for (i, (id, fitness, addr)) in m.peer_details().into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"id\":\"{}\",\"addr\":\"{}\",\"fitness\":{}}}",
                snapshot::escape_json_string(&id),
                snapshot::escape_json_string(&addr),
                fitness
            ));
        }
    }
    out.push_str("]}");
    (200, out)
}

fn endpoint_mesh_broadcast(body: &str, vm: Arc<Mutex<VM>>) -> (u16, String) {
    let expr = match json_get_string(body, "expr") {
        Some(s) => s,
        None => return (400, error_json("missing or invalid \"expr\" field")),
    };
    let vm = vm.lock().unwrap();
    match vm.mesh.as_ref() {
        Some(m) => {
            m.send_sexp(&expr);
            (200, "{\"broadcast\":true}".to_string())
        }
        None => (503, error_json("mesh is not running")),
    }
}

// ---------------------------------------------------------------------------
// Minimal JSON helpers (hand-rolled — matches the aesthetic of snapshot.rs)
// ---------------------------------------------------------------------------

fn error_json(msg: &str) -> String {
    format!("{{\"error\":\"{}\"}}", snapshot::escape_json_string(msg))
}

fn json_object_str(key: &str, value: &str) -> String {
    format!(
        "{{\"{}\":\"{}\"}}",
        snapshot::escape_json_string(key),
        snapshot::escape_json_string(value)
    )
}

/// Extract a string-valued field from a flat JSON object. Handles
/// whitespace, common escape sequences, and ignores other fields.
/// Good enough for the one-or-two-key bodies this bridge accepts;
/// not a full JSON parser.
fn json_get_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let key_pos = body.find(&needle)?;
    let after_key = &body[key_pos + needle.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = &after_key[colon_pos + 1..];
    let mut chars = after_colon.chars();
    let mut c = chars.next()?;
    while c.is_whitespace() {
        c = chars.next()?;
    }
    if c != '"' {
        return None;
    }
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            other => out.push(other),
        }
    }
    None
}

/// Decode a percent-encoded URL path segment. ASCII-only; high bytes
/// pass through as latin-1 chars. Forth word names are ASCII.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            let v = u8::from_str_radix(hex, 16).ok()?;
            out.push(v);
            i += 3;
        } else if b == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_string_field() {
        let body = r#"{"source": "2 3 + ."}"#;
        assert_eq!(json_get_string(body, "source"), Some("2 3 + .".into()));
    }

    #[test]
    fn parse_escapes() {
        let body = r#"{"s":"a\nb\"c"}"#;
        assert_eq!(json_get_string(body, "s"), Some("a\nb\"c".into()));
    }

    #[test]
    fn missing_field_returns_none() {
        let body = r#"{"other":"x"}"#;
        assert_eq!(json_get_string(body, "source"), None);
    }

    #[test]
    fn decode_percent() {
        assert_eq!(percent_decode("HELLO").as_deref(), Some("HELLO"));
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("a+b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("%GG"), None);
    }

    #[test]
    fn find_headers_delimiter() {
        let data = b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody";
        let pos = find_header_end(data).unwrap();
        assert_eq!(&data[..pos], b"GET / HTTP/1.1\r\nHost: x");
        assert_eq!(&data[pos + 4..], b"body");
    }
}
