use anyhow::Result;
use std::process::Command;

/// Get the output directory from ragu
pub fn get_output_dir(working_dir: &str) -> Result<String> {
    let output = Command::new("ragu")
        .arg("output-dir")
        .current_dir(working_dir)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("ragu output-dir failed: {}", stderr));
    }

    let output_dir = String::from_utf8(output.stdout)?;
    let trimmed = output_dir.trim().to_string();

    Ok(trimmed)
}

/// Get source globs from ragu
pub fn get_sources(working_dir: &str) -> Result<Vec<String>> {
    let output = Command::new("ragu")
        .arg("sources")
        .current_dir(working_dir)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!("ragu sources failed: {}", stderr));
    }

    let sources = String::from_utf8(output.stdout)?;
    let globs: Vec<String> = sources
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    Ok(globs)
}

/// Initialize configuration using ragu
pub fn init_config(working_dir: &str) -> Result<crate::types::Config> {
    let output_dir = get_output_dir(working_dir)?;
    let source_globs = get_sources(working_dir)?;

    Ok(crate::types::Config {
        output_dir,
        source_globs,
        formatter: crate::types::Formatter::PursTidy,
        fast_rebuild_on_save: true,
    })
}
