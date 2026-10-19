//! `chorus-hook <url>`: the command Codex runs for each hook (D37). It reads the hook's JSON from
//! stdin, posts it to Chorus's listener at `<url>` (loopback only) with the session's token from
//! `CHORUS_HOOK_TOKEN`, and prints the listener's reply only when there is one: a hook's stdout is
//! a decision channel (LESSONS L2). The token never touches argv, which other processes can read.
//!
//! It always exits 0. Codex shows a warning for a failing hook and Chorus only listens, so a missed
//! event is better than a session full of warnings.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const MAX_PAYLOAD: u64 = 8 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// `http://127.0.0.1:<port>/<path>` as the address and path. Anything else is refused, so the
/// token can only ever go to this machine.
fn parse_url(url: &str) -> Option<(SocketAddr, String)> {
    let rest = url.strip_prefix("http://127.0.0.1:")?;
    let (port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    if !path.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b)) {
        return None;
    }
    Some((SocketAddr::from(([127, 0, 0, 1], port)), format!("/{path}")))
}

/// The body of a 2xx answer, or `None`.
fn reply_body(response: &[u8]) -> Option<&[u8]> {
    let head_end = response.windows(4).position(|w| w == b"\r\n\r\n")?;
    let status_line = response[..head_end].split(|b| *b == b'\n').next()?;
    let code = std::str::from_utf8(status_line).ok()?.split_whitespace().nth(1)?;
    code.starts_with('2').then_some(&response[head_end + 4..])
}

fn relay(url: &str, token: &str, payload: &[u8]) -> Option<Vec<u8>> {
    let (addr, path) = parse_url(url)?;
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(REPLY_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(REPLY_TIMEOUT)).ok()?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    // One write, so the listener has the whole request before it answers.
    let mut request = head.into_bytes();
    request.extend_from_slice(payload);
    stream.write_all(&request).ok()?;
    // Keep what arrived even if the connection then drops: Windows resets a socket closed with
    // unread data.
    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    while (response.len() as u64) < MAX_PAYLOAD {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => response.extend_from_slice(&chunk[..n]),
        }
    }
    let body = reply_body(&response)?;
    (!body.iter().all(u8::is_ascii_whitespace)).then(|| body.to_vec())
}

fn main() {
    let url = std::env::args().nth(1).unwrap_or_default();
    let token = std::env::var("CHORUS_HOOK_TOKEN").unwrap_or_default();
    let mut payload = Vec::new();
    if token.is_empty() || std::io::stdin().take(MAX_PAYLOAD).read_to_end(&mut payload).is_err() {
        return;
    }
    if let Some(reply) = relay(&url, &token, &payload) {
        let mut out = std::io::stdout();
        let _ = out.write_all(&reply).and_then(|()| out.flush());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn only_loopback_urls_are_taken() {
        assert_eq!(parse_url("http://127.0.0.1:4100/hook/12").unwrap().1, "/hook/12");
        assert_eq!(parse_url("http://127.0.0.1:4100/hook/12").unwrap().0.port(), 4100);
        for bad in ["http://evil.example:80/h", "https://127.0.0.1:1/h", "http://127.0.0.1:0/h", "http://127.0.0.1:9/h?x=1", ""] {
            assert!(parse_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn only_a_successful_non_empty_reply_is_printed() {
        assert_eq!(reply_body(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"), Some(&b"{}"[..]));
        assert_eq!(reply_body(b"HTTP/1.1 204 No Content\r\n\r\n"), Some(&b""[..]));
        assert_eq!(reply_body(b"HTTP/1.1 401 Unauthorized\r\n\r\nno"), None);
        assert_eq!(reply_body(b"garbage"), None);
    }

    #[test]
    fn it_posts_the_payload_with_the_token_in_a_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while !got.ends_with(b"{\"a\":1}") {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"decision\":1}\n").unwrap();
            String::from_utf8_lossy(&got).into_owned()
        });
        let reply = relay(&format!("http://127.0.0.1:{port}/hook/3"), "tok-123", b"{\"a\":1}").unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /hook/3 HTTP/1.1\r\n"));
        assert!(request.contains("Authorization: Bearer tok-123\r\n"));
        assert!(request.ends_with("{\"a\":1}"));
        assert_eq!(reply, b"{\"decision\":1}\n");
    }
}
