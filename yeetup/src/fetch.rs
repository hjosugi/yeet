//! Downloading and integrity checking.
//!
//! Every byte that ends up on disk is checked against the published
//! `SHA256SUMS` manifest before it is unpacked, so a truncated download or a
//! substituted archive is rejected rather than installed.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Result;
use crate::release::USER_AGENT;

/// Ceiling for a single download. The Windows bundle ships the whole GTK
/// runtime, so the limit has to be generous while still bounding a runaway
/// response.
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const PROGRESS_STEP_BYTES: u64 = 4 * 1024 * 1024;

pub fn agent() -> ureq::Agent {
    ureq::Agent::new_with_defaults()
}

pub fn get_text(agent: &ureq::Agent, url: &str) -> Result<String> {
    Ok(agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()?
        .body_mut()
        .read_to_string()?)
}

/// Stream `url` to `destination`, returning its lowercase SHA-256.
///
/// Streamed rather than buffered so a large bundle never has to fit in memory,
/// and hashed while streaming so the file is read only once.
pub fn download_to(agent: &ureq::Agent, url: &str, destination: &Path) -> Result<String> {
    let mut response = agent.get(url).header("User-Agent", USER_AGENT).call()?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD_BYTES)
        .reader();
    let mut writer = BufWriter::new(File::create(destination)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    let mut announced = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        writer.write_all(&buffer[..read])?;
        total += read as u64;
        if total - announced >= PROGRESS_STEP_BYTES {
            announced = total;
            println!("  {} MiB", total / (1024 * 1024));
        }
    }
    writer.flush()?;
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn verify(actual: &str, expected: &str, file: &str) -> Result<()> {
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    Err(format!(
        "{file} failed its checksum: expected {expected}, got {actual}. \
         The download was not installed."
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_comparison_ignores_case_but_not_content() {
        assert!(verify("ABCD", "abcd", "yeet.tar.gz").is_ok());

        let error = verify("abcd", "ef01", "yeet.tar.gz")
            .unwrap_err()
            .to_string();
        assert!(error.contains("was not installed"), "{error}");
    }
}
