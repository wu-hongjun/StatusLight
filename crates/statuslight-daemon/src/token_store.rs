//! Encrypted token storage for Slack credentials.
//!
//! Tokens are encrypted with AES-256-GCM using a machine-derived key
//! (SHA-256 of hostname + username + application salt) and stored in
//! `~/.config/statuslight/tokens.enc`. This prevents casual plaintext
//! exposure without requiring OS keychain popups.

use std::collections::HashMap;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};

const APP_SALT: &str = "statuslight-token-encryption";

/// Returns the path to the encrypted token file.
fn token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("statuslight").join("tokens.enc"))
}

/// Derive a 256-bit key from machine identity.
fn derive_key() -> [u8; 32] {
    let hostname = std::process::Command::new("hostname")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string());

    // On Unix, use $USER; falls back to USERNAME on Windows.
    let username = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(username.as_bytes());
    hasher.update(APP_SALT.as_bytes());
    hasher.finalize().into()
}

/// Load all stored tokens from the encrypted file.
pub fn load_tokens() -> Result<HashMap<String, String>> {
    let path = token_path().context("cannot determine config dir")?;
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let encoded = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let data = general_purpose::STANDARD
        .decode(encoded.trim())
        .context("failed to decode token file")?;

    if data.len() < 12 {
        anyhow::bail!("token file too short");
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key).context("failed to create cipher")?;

    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        anyhow::anyhow!("failed to decrypt tokens (machine identity may have changed)")
    })?;

    let tokens: HashMap<String, String> =
        serde_json::from_slice(&plaintext).context("failed to parse decrypted tokens")?;

    Ok(tokens)
}

/// Encrypt and write `tokens` to the store file (overwrites the file).
fn write_tokens(tokens: &HashMap<String, String>) -> Result<()> {
    let path = token_path().context("cannot determine config dir")?;

    let plaintext = serde_json::to_vec(tokens).context("failed to serialize tokens")?;

    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key).context("failed to create cipher")?;

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|_| anyhow::anyhow!("failed to encrypt tokens"))?;

    // Prepend nonce to ciphertext.
    let mut output = Vec::with_capacity(12 + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    let encoded = general_purpose::STANDARD.encode(&output);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config dir {}", parent.display()))?;
    }

    // Write with restricted permissions from the start to avoid TOCTOU.
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("failed to create token file {}", path.display()))?;
        file.write_all(encoded.as_bytes())
            .with_context(|| format!("failed to write token file {}", path.display()))?;
    }

    #[cfg(not(unix))]
    {
        std::fs::write(&path, &encoded)
            .with_context(|| format!("failed to write token file {}", path.display()))?;
    }

    Ok(())
}

/// Store tokens to the encrypted file (merges with existing tokens).
///
/// Passing an empty string for a value removes that key.
pub fn store_tokens(new_tokens: &HashMap<String, String>) -> Result<()> {
    // Merge with existing tokens (propagate errors to avoid silently losing data).
    let mut tokens = load_tokens().context("failed to load existing tokens for merge")?;
    for (k, v) in new_tokens {
        if v.is_empty() {
            tokens.remove(k);
        } else {
            tokens.insert(k.clone(), v.clone());
        }
    }
    write_tokens(&tokens)
}
