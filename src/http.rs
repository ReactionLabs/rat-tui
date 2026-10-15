//! Minimal synchronous HTTP/1.1 GET over a raw TcpStream.
//! No reqwest, no tokio: just enough to poll a local GoChain node.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const MAX_BODY: usize = 8 * 1024 * 1024; // mirror the node's own sync bound spirit

/// GET `url` (http:// only) and return the response body as a String.
/// Any failure (connect, status, parse) is an Err; callers treat it as "node quiet".
pub fn get(url: &str, timeout: Duration) -> io::Result<String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| err("only http:// URLs are supported"))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = authority
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| err("could not resolve host"))?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    let mut raw = Vec::new();
    let mut buf = [0u8; 16384];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > MAX_BODY + 65536 {
            return Err(err("response too large"));
        }
    }

    let split = find_subslice(&raw, b"\r\n\r\n").ok_or_else(|| err("malformed HTTP response"))?;
    let head = String::from_utf8_lossy(&raw[..split]).to_lowercase();
    let status_line = head.lines().next().unwrap_or("");
    if !status_line.contains(" 200") {
        return Err(err(&format!("unexpected status: {status_line}")));
    }
    let body = &raw[split + 4..];
    if head.contains("transfer-encoding: chunked") {
        Ok(String::from_utf8_lossy(&dechunk(body)).into_owned())
    } else {
        Ok(String::from_utf8_lossy(body).into_owned())
    }
}

fn dechunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = data;
    loop {
        let Some(end) = find_subslice(rest, b"\r\n") else {
            break;
        };
        let size_str = String::from_utf8_lossy(&rest[..end]).trim().to_string();
        let Ok(size) = usize::from_str_radix(size_str.split(';').next().unwrap_or(""), 16) else {
            break;
        };
        if size == 0 {
            break;
        }
        let start = end + 2;
        if start + size > rest.len() {
            break;
        }
        out.extend_from_slice(&rest[start..start + size]);
        rest = &rest[start + size..];
        if rest.starts_with(b"\r\n") {
            rest = &rest[2..];
        }
    }
    out
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

fn err(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Other, msg.to_string())
}
