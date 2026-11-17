//! A tiny HTTP/1.1 server for tests. Each connection is answered once by a handler, which can
//! reply in one piece, in delayed pieces (to exercise streaming), or stream and then hold the
//! connection open (to exercise cancellation).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug)]
pub struct Req {
    pub method: String,
    /// Path with its query string.
    pub path: String,
    /// Header names are lower-cased.
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Req {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub enum Body {
    Bytes(Vec<u8>),
    /// Pieces sent one after another, each after its delay in milliseconds; then the connection closes.
    Stream(Vec<(u64, Vec<u8>)>),
    /// As `Stream`, but the connection then stays open until the client goes away.
    StreamThenHang(Vec<(u64, Vec<u8>)>),
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl Resp {
    pub fn json(value: serde_json::Value) -> Resp {
        Resp::bytes(200, "application/json", value.to_string().into_bytes())
    }

    pub fn error(status: u16, value: serde_json::Value) -> Resp {
        Resp::bytes(status, "application/json", value.to_string().into_bytes())
    }

    pub fn bytes(status: u16, content_type: &str, body: Vec<u8>) -> Resp {
        Resp { status, headers: vec![("Content-Type".into(), content_type.into())], body: Body::Bytes(body) }
    }

    pub fn stream(content_type: &str, pieces: Vec<(u64, String)>) -> Resp {
        let pieces = pieces.into_iter().map(|(d, p)| (d, p.into_bytes())).collect();
        Resp { status: 200, headers: vec![("Content-Type".into(), content_type.into())], body: Body::Stream(pieces) }
    }

    pub fn stream_then_hang(content_type: &str, pieces: Vec<(u64, String)>) -> Resp {
        let pieces = pieces.into_iter().map(|(d, p)| (d, p.into_bytes())).collect();
        Resp { status: 200, headers: vec![("Content-Type".into(), content_type.into())], body: Body::StreamThenHang(pieces) }
    }

    pub fn header(mut self, name: &str, value: &str) -> Resp {
        self.headers.push((name.into(), value.into()));
        self
    }
}

pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Req>>>,
    /// How many held-open responses saw their client disconnect.
    pub disconnects: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub fn last(&self, path_prefix: &str) -> Req {
        self.requests.lock().unwrap().iter().rev().find(|r| r.path.starts_with(path_prefix)).cloned().expect("no such request")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let Some(head_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut lines = head.lines();
        let mut first = lines.next()?.split_whitespace();
        let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
        let headers: HashMap<String, String> =
            lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
        let length: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        while buf.len() < head_end + 4 + length {
            let n = socket.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let body = String::from_utf8_lossy(&buf[head_end + 4..head_end + 4 + length]).into_owned();
        return Some(Req { method, path, headers, body });
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// Start a server on a free loopback port. It stops when the returned value is dropped.
pub async fn serve(handler: impl Fn(&Req) -> Resp + Send + Sync + 'static) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let disconnects = Arc::new(AtomicUsize::new(0));
    let handler = Arc::new(handler);
    let (log, gone) = (requests.clone(), disconnects.clone());
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let (handler, log, gone) = (handler.clone(), log.clone(), gone.clone());
            tokio::spawn(async move {
                let Some(req) = read_request(&mut socket).await else { return };
                let resp = handler(&req);
                log.lock().unwrap().push(req);
                let mut head = format!("HTTP/1.1 {} {}\r\nConnection: close\r\n", resp.status, reason(resp.status));
                for (k, v) in &resp.headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                match resp.body {
                    Body::Bytes(bytes) => {
                        head.push_str(&format!("Content-Length: {}\r\n\r\n", bytes.len()));
                        let _ = socket.write_all(head.as_bytes()).await;
                        let _ = socket.write_all(&bytes).await;
                    }
                    body => {
                        let (pieces, hang) = match body {
                            Body::Stream(p) => (p, false),
                            Body::StreamThenHang(p) => (p, true),
                            Body::Bytes(_) => unreachable!(),
                        };
                        head.push_str("\r\n");
                        if socket.write_all(head.as_bytes()).await.is_err() {
                            return;
                        }
                        for (delay, piece) in pieces {
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                            if socket.write_all(&piece).await.is_err() || socket.flush().await.is_err() {
                                gone.fetch_add(1, Ordering::SeqCst);
                                return;
                            }
                        }
                        if hang {
                            // Wait for the client to close its side (read returns 0 or an error).
                            let mut sink = [0u8; 64];
                            let closed = tokio::time::timeout(Duration::from_secs(20), async {
                                loop {
                                    match socket.read(&mut sink).await {
                                        Ok(0) | Err(_) => break,
                                        Ok(_) => {}
                                    }
                                }
                            })
                            .await;
                            if closed.is_ok() {
                                gone.fetch_add(1, Ordering::SeqCst);
                            }
                            return;
                        }
                    }
                }
                let _ = socket.shutdown().await;
            });
        }
    });
    Server { url, requests, disconnects, task }
}
