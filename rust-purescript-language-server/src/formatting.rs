use crate::types::Formatter;
use anyhow::Result;
use lsp_types::{Position, Range, TextEdit};
use tokio::process::Command;

/// Format document content using the specified formatter
pub async fn format_document_content(
    content: &str,
    formatter: &Formatter,
) -> Result<Option<Vec<TextEdit>>> {
    let formatted_content = match formatter {
        Formatter::PursFmt => format_with("pursfmt", content).await?,
        Formatter::PursTidy => format_with("purs-tidy", content).await?,
    };

    if let Some(formatted) = formatted_content {
        let full_range = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: u32::MAX,
                character: 0,
            },
        };

        Ok(Some(vec![TextEdit {
            range: full_range,
            new_text: formatted,
        }]))
    } else {
        Ok(None)
    }
}

async fn format_with(command: &str, content: &str) -> Result<Option<String>> {
    // purs-tidy format expects stdin
    let mut child = Command::new(command)
        .arg("format")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Write content to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(content.as_bytes()).await?;
        stdin.flush().await?;
        drop(stdin);
    }

    let output = child.wait_with_output().await?;

    if output.status.success() {
        let formatted = String::from_utf8(output.stdout)?;
        Ok(Some(formatted))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        anyhow::bail!("{} failed: stderr={}, stdout={}", command, stderr, stdout)
    }
}
