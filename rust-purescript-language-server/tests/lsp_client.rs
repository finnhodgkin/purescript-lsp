use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

pub struct TestLspClient {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i32,
}

impl TestLspClient {
    pub fn new(workspace_path: &Path) -> Result<Self> {
        // Try multiple possible binary paths
        let possible_paths = [
            "/Users/finnhodgkin/purescript-language-server/rust-purescript-language-server/target/debug/rust-purescript-language-server",
            "./target/debug/rust-purescript-language-server",
            "rust-purescript-language-server",
        ];

        let binary_path = possible_paths
            .iter()
            .find(|path| {
                let exists = std::path::Path::new(path).exists();
                println!("Checking path: {} -> exists: {}", path, exists);
                exists || *path == &"rust-purescript-language-server"
            })
            .ok_or_else(|| {
                anyhow::anyhow!("Could not find rust-purescript-language-server binary")
            })?;

        println!("Selected binary path: {}", binary_path);

        let mut process = Command::new(binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(workspace_path)
            .spawn()
            .context("Failed to spawn rust-purescript-language-server")?;

        let stdin = process.stdin.take().unwrap();
        let stdout = BufReader::new(process.stdout.take().unwrap());

        Ok(TestLspClient {
            process,
            stdin,
            stdout,
            next_id: 1,
        })
    }

    pub fn initialize(&mut self, workspace_path: &Path) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "initialize",
            "params": {
                "capabilities": {
                    "textDocument": {
                        "synchronization": {
                            "dynamicRegistration": false,
                            "willSave": false,
                            "willSaveWaitUntil": false,
                            "didSave": true
                        },
                        "publishDiagnostics": {
                            "relatedInformation": true
                        },
                        "formatting": {
                            "dynamicRegistration": false
                        },
                        "codeAction": {
                            "dynamicRegistration": false
                        }
                    }
                },
                "rootUri": format!("file://{}", workspace_path.display())
            }
        });

        self.next_id += 1;
        self.send_message(&request)?;
        self.wait_for_response(self.next_id - 1)
    }

    pub fn send_notification(&mut self, method: &str, params: Value) -> Result<()> {
        let notification = if params.is_null() {
            json!({
                "jsonrpc": "2.0",
                "method": method
            })
        } else {
            json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params
            })
        };

        self.send_message(&notification)
    }

    pub fn send_request(&mut self, method: &str, params: Value) -> Result<Value> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params
        });

        self.next_id += 1;
        self.send_message(&request)?;
        self.wait_for_response(self.next_id - 1)
    }

    pub fn shutdown(&mut self) -> Result<()> {
        // Send shutdown request
        let shutdown_request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "shutdown"
        });

        self.next_id += 1;
        self.send_message(&shutdown_request)?;

        // Wait for shutdown response with timeout
        let shutdown_response =
            self.wait_for_response_with_timeout(self.next_id - 1, Duration::from_secs(5))?;
        println!("Shutdown response: {:?}", shutdown_response);

        // Send exit notification
        let exit_notification = json!({
            "jsonrpc": "2.0",
            "method": "exit"
        });

        self.send_message(&exit_notification)?;

        // Give the process a moment to exit gracefully
        std::thread::sleep(Duration::from_millis(100));

        // If the process is still running, kill it
        if let Ok(Some(_)) = self.process.try_wait() {
            // Process already exited
        } else {
            // Process still running, kill it
            let _ = self.process.kill();
        }

        // Wait for process to exit
        let status = self.process.wait()?;
        println!("Process exit status: {:?}", status);

        Ok(())
    }

    fn send_message(&mut self, message: &Value) -> Result<()> {
        let message_str = serde_json::to_string(message)?;
        let content_length = message_str.len();
        let lsp_message = format!("Content-Length: {}\r\n\r\n{}", content_length, message_str);

        self.stdin.write_all(lsp_message.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> Result<Value> {
        let mut content_length = None;
        let mut line = String::new();

        // Read headers with timeout
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(10) {
                anyhow::bail!("Timeout reading message headers");
            }

            line.clear();
            self.stdout.read_line(&mut line)?;
            let line = line.trim();

            if line.is_empty() {
                break;
            }

            if line.starts_with("Content-Length: ") {
                content_length = Some(
                    line.strip_prefix("Content-Length: ")
                        .unwrap()
                        .parse::<usize>()?,
                );
            }
        }

        let content_length = content_length.context("Missing Content-Length header")?;

        // Read message body
        let mut buffer = vec![0; content_length];
        self.stdout.read_exact(&mut buffer)?;

        let message_str = String::from_utf8(buffer)?;
        let message: Value = serde_json::from_str(&message_str)?;

        Ok(message)
    }

    fn wait_for_response(&mut self, expected_id: i32) -> Result<Value> {
        loop {
            let message = self.read_message()?;

            if let Some(id) = message.get("id").and_then(|id| id.as_i64()) {
                if id == expected_id as i64 {
                    if let Some(result) = message.get("result") {
                        return Ok(result.clone());
                    } else if let Some(error) = message.get("error") {
                        anyhow::bail!("LSP error: {}", error);
                    }
                }
            }
        }
    }

    fn wait_for_response_with_timeout(
        &mut self,
        expected_id: i32,
        timeout: Duration,
    ) -> Result<Value> {
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                anyhow::bail!("Timeout waiting for response with id {}", expected_id);
            }

            let message = self.read_message()?;

            if let Some(id) = message.get("id").and_then(|id| id.as_i64()) {
                if id == expected_id as i64 {
                    if let Some(result) = message.get("result") {
                        return Ok(result.clone());
                    } else if let Some(error) = message.get("error") {
                        anyhow::bail!("LSP error: {}", error);
                    }
                }
            }
        }
    }
}

impl Drop for TestLspClient {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}
