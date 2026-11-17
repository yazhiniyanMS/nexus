//! Debug-only UI test bridge. Compiled out of release builds, and inactive unless
//! `NEXUS_E2E_PORT` is set. It lets a test script run JavaScript inside the real
//! window over a loopback socket, so features can be exercised through the actual interface.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Listener, Manager, Runtime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<String>>>>;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let Some(port) = std::env::var("NEXUS_E2E_PORT").ok().and_then(|p| p.parse::<u16>().ok()) else { return };
    let pending: Pending = Arc::default();
    let results = pending.clone();
    app.listen("e2e-result", move |event| {
        let id = serde_json::from_str::<Value>(event.payload()).ok().and_then(|v| v.get("id")?.as_u64());
        if let Some(tx) = id.and_then(|id| results.lock().unwrap().remove(&id)) {
            let _ = tx.send(event.payload().to_string());
        }
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await else {
            eprintln!("e2e bridge: could not bind 127.0.0.1:{port}");
            return;
        };
        eprintln!("e2e bridge listening on 127.0.0.1:{port} (debug build only)");
        while let Ok((stream, _)) = listener.accept().await {
            tauri::async_runtime::spawn(serve(stream, app.clone(), pending.clone()));
        }
    });
}

/// Read one HTTP request whose body is a JavaScript function body, run it in the window, reply with JSON.
async fn serve<R: Runtime>(mut stream: TcpStream, app: AppHandle<R>, pending: Pending) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let body = loop {
        let Ok(n) = stream.read(&mut chunk).await else { return };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        let Some(split) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
        let head = String::from_utf8_lossy(&buf[..split]).to_lowercase();
        let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
        if buf.len() >= split + 4 + len {
            break String::from_utf8_lossy(&buf[split + 4..split + 4 + len]).into_owned();
        }
    };

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    pending.lock().unwrap().insert(id, tx);
    let script = format!(
        "(async()=>{{let r;try{{r={{ok:true,value:await (async()=>{{{body}\n}})()}}}}catch(e){{r={{ok:false,error:String(e)}}}}\
         window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'e2e-result',payload:{{id:{id},...r}}}})}})()"
    );
    let reply = match app.get_webview_window("main").map(|w| w.eval(&script)) {
        Some(Ok(())) => match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(json)) => json,
            _ => r#"{"ok":false,"error":"timed out waiting for the window"}"#.to_string(),
        },
        _ => r#"{"ok":false,"error":"main window not available"}"#.to_string(),
    };
    pending.lock().unwrap().remove(&id);
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        reply.len(),
        reply
    );
    let _ = stream.write_all(response.as_bytes()).await;
}
