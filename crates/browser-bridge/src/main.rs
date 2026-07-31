use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use browser_bridge::{append_audit_record, read_native_message, socket_path, write_native_message};
use serde_json::{json, Value};

type Pending = Arc<Mutex<HashMap<String, mpsc::Sender<Value>>>>;

fn main() -> io::Result<()> {
    let socket = socket_path();
    if let Some(parent) = socket.parent() {
        fs::create_dir_all(parent)?;
    }
    if socket.exists() {
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    let (chrome_tx, chrome_rx) = mpsc::channel::<Value>();
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let sequence = Arc::new(AtomicU64::new(1));

    thread::spawn(move || {
        let stdout = io::stdout();
        let mut output = stdout.lock();
        for message in chrome_rx {
            if write_native_message(&mut output, &message).is_err() {
                break;
            }
        }
    });

    let socket_pending = Arc::clone(&pending);
    thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(stream) = connection else { continue };
            let chrome_tx = chrome_tx.clone();
            let pending = Arc::clone(&socket_pending);
            let sequence = Arc::clone(&sequence);
            thread::spawn(move || handle_client(stream, &chrome_tx, &pending, &sequence));
        }
    });

    let stdin = io::stdin();
    let mut input = stdin.lock();
    while let Some(response) = read_native_message(&mut input)? {
        let Some(request_id) = response.get("requestId").and_then(Value::as_str) else {
            continue;
        };
        if let Some(sender) = pending
            .lock()
            .expect("pending lock poisoned")
            .remove(request_id)
        {
            let _ = sender.send(response);
        }
    }
    Ok(())
}

fn handle_client(
    mut stream: std::os::unix::net::UnixStream,
    chrome_tx: &mpsc::Sender<Value>,
    pending: &Pending,
    sequence: &AtomicU64,
) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let Ok(mut command) = serde_json::from_str::<Value>(&line) else {
        let _ = stream.write_all(b"{\"ok\":false,\"error\":\"invalid JSON command\"}");
        return;
    };
    let request_id = format!("rt-{}", sequence.fetch_add(1, Ordering::Relaxed));
    command["requestId"] = Value::String(request_id.clone());

    let (response_tx, response_rx) = mpsc::channel();
    pending
        .lock()
        .expect("pending lock poisoned")
        .insert(request_id.clone(), response_tx);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs());
    let _ = append_audit_record(
        &json!({"timestamp": timestamp, "direction": "request", "command": command}),
    );
    if chrome_tx.send(command).is_err() {
        let _ = stream.write_all(b"{\"ok\":false,\"error\":\"Chrome extension disconnected\"}");
        return;
    }
    let response = response_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap_or_else(|_| {
            pending
                .lock()
                .expect("pending lock poisoned")
                .remove(&request_id);
            json!({"requestId": request_id, "ok": false, "error": "Chrome command timed out"})
        });
    let _ = append_audit_record(
        &json!({"timestamp": timestamp, "direction": "response", "response": response}),
    );
    if let Ok(bytes) = serde_json::to_vec(&response) {
        let _ = stream.write_all(&bytes);
    }
}
