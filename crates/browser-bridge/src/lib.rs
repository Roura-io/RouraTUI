use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde_json::Value;

pub const MAX_NATIVE_MESSAGE_BYTES: usize = 1024 * 1024;

#[must_use]
pub fn socket_path() -> PathBuf {
    env::var_os("ROURATUI_BROWSER_SOCKET").map_or_else(
        || {
            let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
            PathBuf::from(home)
                .join(".rouratui")
                .join("browser-control.sock")
        },
        PathBuf::from,
    )
}

#[must_use]
pub fn audit_log_path() -> PathBuf {
    let home = env::var_os("HOME").unwrap_or_else(|| ".".into());
    PathBuf::from(home)
        .join(".rouratui")
        .join("browser-audit.jsonl")
}

pub fn send_command(command: &Value) -> io::Result<Value> {
    let mut stream = UnixStream::connect(socket_path())?;
    let bytes = serde_json::to_vec(command).map_err(io::Error::other)?;
    if bytes.len() > MAX_NATIVE_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "browser command exceeds 1 MiB",
        ));
    }
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut response = Vec::new();
    stream
        .take(MAX_NATIVE_MESSAGE_BYTES as u64)
        .read_to_end(&mut response)?;
    serde_json::from_slice(&response).map_err(io::Error::other)
}

pub fn read_native_message(input: &mut impl Read) -> io::Result<Option<Value>> {
    let mut length = [0_u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_NATIVE_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native message exceeds 1 MiB",
        ));
    }
    let mut payload = vec![0; length];
    input.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(io::Error::other)
}

pub fn write_native_message(output: &mut impl Write, message: &Value) -> io::Result<()> {
    let payload = serde_json::to_vec(message).map_err(io::Error::other)?;
    if payload.len() > MAX_NATIVE_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native message exceeds 1 MiB",
        ));
    }
    let length = u32::try_from(payload.len()).map_err(io::Error::other)?;
    output.write_all(&length.to_le_bytes())?;
    output.write_all(&payload)?;
    output.flush()
}

pub fn append_audit_record(record: &Value) -> io::Result<()> {
    let path = audit_log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, record).map_err(io::Error::other)?;
    file.write_all(b"\n")
}

pub fn is_consequential(command: &Value) -> bool {
    let kind = command
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(kind, "click" | "type") {
        return false;
    }
    command
        .get("consequential")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{is_consequential, read_native_message, write_native_message};
    use serde_json::json;

    #[test]
    fn native_message_round_trip() {
        let original = json!({"requestId": "r1", "type": "status"});
        let mut bytes = Vec::new();
        write_native_message(&mut bytes, &original).expect("encode");
        let decoded = read_native_message(&mut bytes.as_slice()).expect("decode");
        assert_eq!(decoded, Some(original));
    }

    #[test]
    fn only_explicit_mutating_commands_are_consequential() {
        assert!(is_consequential(
            &json!({"type": "click", "consequential": true})
        ));
        assert!(!is_consequential(&json!({"type": "snapshot"})));
        assert!(!is_consequential(&json!({"type": "click"})));
    }
}
