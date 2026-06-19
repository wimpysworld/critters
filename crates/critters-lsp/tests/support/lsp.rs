use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct LspClient {
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<io::Result<Value>>,
    next_id: u64,
    last_message: Option<Value>,
}

impl LspClient {
    pub fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_critters-lsp"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start critters-lsp binary");

        let stdin = child.stdin.take().expect("child stdin to be piped");
        let stdout = child.stdout.take().expect("child stdout to be piped");
        let (sender, messages) = mpsc::channel();

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(message)) => {
                        if sender.send(Ok(message)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });

        Self {
            child,
            stdin: Some(stdin),
            messages,
            next_id: 1,
            last_message: None,
        }
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;

        let mut message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }

        self.send(message);
        self.wait_for_response(id)
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        let mut message = json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }

        self.send(message);
    }

    pub fn initialize(&mut self) -> Value {
        self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": null,
                "capabilities": {},
                "workspaceFolders": [],
            }),
        )
    }

    pub fn shutdown_and_exit(mut self) -> Value {
        let response = self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        drop(self.stdin.take());
        self.wait_for_exit();
        response
    }

    pub fn wait_for_diagnostics(&mut self, uri: &str, version: Option<i64>) -> Value {
        self.wait_for_message(|message| {
            if message.get("method").and_then(Value::as_str)
                != Some("textDocument/publishDiagnostics")
            {
                return false;
            }

            let Some(params) = message.get("params") else {
                return false;
            };
            if params.get("uri").and_then(Value::as_str) != Some(uri) {
                return false;
            }

            match version {
                Some(version) => params.get("version").and_then(Value::as_i64) == Some(version),
                None => true,
            }
        })
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).expect("serialise JSON-RPC message");
        let stdin = self.stdin.as_mut().expect("child stdin to be open");
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write LSP frame header");
        stdin.write_all(&body).expect("write LSP frame body");
        stdin.flush().expect("flush LSP frame");
    }

    fn wait_for_response(&mut self, id: u64) -> Value {
        self.wait_for_message(|message| message.get("id").and_then(Value::as_u64) == Some(id))
    }

    fn wait_for_message(&mut self, mut matches: impl FnMut(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT_TIMEOUT;

        loop {
            let now = Instant::now();
            if now >= deadline {
                panic!(
                    "timed out waiting for LSP message after {:?}; last message: {}",
                    WAIT_TIMEOUT,
                    format_message(self.last_message.as_ref())
                );
            }

            match self
                .messages
                .recv_timeout(deadline.saturating_duration_since(now))
            {
                Ok(Ok(message)) => {
                    self.last_message = Some(message.clone());
                    if matches(&message) {
                        return message;
                    }
                }
                Ok(Err(error)) => {
                    panic!(
                        "failed to read LSP message: {error}; last message: {}",
                        format_message(self.last_message.as_ref())
                    );
                }
                Err(RecvTimeoutError::Timeout) => {
                    panic!(
                        "timed out waiting for LSP message after {:?}; last message: {}",
                        WAIT_TIMEOUT,
                        format_message(self.last_message.as_ref())
                    );
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!(
                        "LSP process closed stdout before target message; last message: {}",
                        format_message(self.last_message.as_ref())
                    );
                }
            }
        }
    }

    fn wait_for_exit(&mut self) {
        let deadline = Instant::now() + EXIT_TIMEOUT;

        loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.success() => return,
                Ok(Some(status)) => panic!("critters-lsp exited with {status}"),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                Ok(None) => panic!("critters-lsp did not exit after {:?}", EXIT_TIMEOUT),
                Err(error) => panic!("failed to wait for critters-lsp: {error}"),
            }
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut content_length = None;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }

        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }

        if let Some(value) = header.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid Content-Length header: {error}"),
                )
            })?);
        }
    }

    let content_length = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length header")
    })?;
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;

    serde_json::from_slice(&body).map(Some).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid JSON-RPC message: {error}"),
        )
    })
}

fn format_message(message: Option<&Value>) -> String {
    message
        .map(Value::to_string)
        .unwrap_or_else(|| "<none>".to_string())
}
