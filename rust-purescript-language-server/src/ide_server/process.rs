use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::sleep;

/// Find an available port by binding to port 0 and letting the OS assign one
pub fn find_available_port() -> Result<u16> {
    let socket =
        std::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0))?;
    let port = socket.local_addr()?.port();
    Ok(port)
}

/// Check if the purs command is available
pub fn validate_purs_command() -> Result<()> {
    let output = Command::new("purs").arg("--version").output();

    match output {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "purs command failed with exit code: {}. stderr: {}",
                    output.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }
        Err(e) => Err(anyhow::anyhow!(
            "purs command not found: {}. Please ensure PureScript is installed and 'purs' is in your PATH",
            e
        )),
    }
}

/// Start the purs ide server process
pub fn start_ide_server(
    working_dir: &str,
    output_dir: &str,
    source_globs: &[String],
    port: u16,
) -> Result<Child> {
    // Note: Command::new doesn't go through a shell, so globs are passed literally
    // This is what we want - the IDE server will expand them itself
    let mut cmd = Command::new("purs");
    cmd.arg("ide")
        .arg("server")
        .arg("-p")
        .arg(port.to_string())
        .arg("--output-directory")
        .arg(output_dir)
        .args(source_globs) // Pass globs as positional arguments
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn()?;
    Ok(child)
}

/// Start IDE server and wait for it to be ready
pub async fn start_ide_server_async(
    working_dir: &str,
    output_dir: &str,
    source_globs: &[String],
) -> Result<(Child, u16)> {
    // Validate purs command exists before attempting to start
    validate_purs_command()?;

    // Find an available port
    let port = find_available_port()?;

    let mut child = start_ide_server(working_dir, output_dir, source_globs, port)?;

    // Capture stdout and stderr for debugging
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let stdout_captured = Arc::new(Mutex::new(String::new()));
    let stderr_captured = Arc::new(Mutex::new(String::new()));

    let stdout_captured_clone = stdout_captured.clone();
    let stderr_captured_clone = stderr_captured.clone();

    // Spawn background tasks to capture output using blocking I/O
    tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let mut captured = stdout_captured_clone.lock().unwrap();
                    captured.push_str(&line);
                    captured.push('\n');
                }
                Err(_) => break,
            }
        }
    });

    tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    let mut captured = stderr_captured_clone.lock().unwrap();
                    captured.push_str(&line);
                    captured.push('\n');
                }
                Err(_) => break,
            }
        }
    });

    // Check if process is still alive
    if let Some(exit_status) = child.try_wait()? {
        let stdout_output = stdout_captured.lock().unwrap().clone();
        let stderr_output = stderr_captured.lock().unwrap().clone();

        return Err(anyhow::anyhow!(
            "IDE server process exited early with status: {}. stdout: '{}', stderr: '{}'",
            exit_status,
            stdout_output,
            stderr_output
        ));
    }

    // Verify the server is listening on the allocated port
    let mut attempts = 0;
    let max_attempts = 50; // 5 seconds max wait

    loop {
        // Check if process is still alive
        if let Some(exit_status) = child.try_wait()? {
            let stdout_output = stdout_captured.lock().unwrap().clone();
            let stderr_output = stderr_captured.lock().unwrap().clone();

            return Err(anyhow::anyhow!(
                "IDE server process exited early with status: {}. stdout: '{}', stderr: '{}'",
                exit_status,
                stdout_output,
                stderr_output
            ));
        }

        // Try to connect to verify the server is up
        if let Ok(_) = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            // eprintln!("IDE server verified on port {}", port);
            return Ok((child, port));
        }

        attempts += 1;
        if attempts >= max_attempts {
            let stdout_output = stdout_captured.lock().unwrap().clone();
            let stderr_output = stderr_captured.lock().unwrap().clone();

            return Err(anyhow::anyhow!(
                "IDE server failed to start within timeout. stdout: '{}', stderr: '{}'",
                stdout_output,
                stderr_output
            ));
        }

        sleep(Duration::from_millis(100)).await;
    }
}
