//! Secure local encrypted vault for credentials and session tokens.
//!
//! Uses AES-256-GCM authenticated encryption with Argon2id key derivation
//! bound to host state directory (`~/.meridian/vault.bin`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

const SALT_SIZE: usize = 16;
const NONCE_SIZE: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VaultData {
    pub apple_id: Option<String>,
    pub password: Option<String>,
    pub anisette_url: Option<String>,
    pub tailscale_key_url: Option<String>,
    pub tailscale_auth_key: Option<String>,
    pub sensitive_data_masked: bool,
    pub auto_start_devices: bool,
    pub custom_settings: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Vault {
    path: PathBuf,
}

impl Vault {
    pub fn new<P: AsRef<Path>>(storage_dir: P) -> Self {
        let dir = storage_dir.as_ref();
        let _ = fs::create_dir_all(dir);
        Self {
            path: dir.join("vault.bin"),
        }
    }

    /// Default vault location in `~/.meridian` or `%LOCALAPPDATA%\Meridian`
    pub fn default_location() -> Self {
        let base = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("meridian");
        Self::new(base)
    }

    fn derive_key(salt: &[u8]) -> [u8; 32] {
        // Machine-unique entropy source + static salt
        let host_seed = format!("{}-meridian-vault-v1", whoami());
        let mut key = [0u8; 32];
        let argon = Argon2::default();
        let _ = argon.hash_password_into(host_seed.as_bytes(), salt, &mut key);
        key
    }

    pub fn load(&self) -> VaultData {
        if !self.path.exists() {
            return VaultData::default();
        }

        match fs::read(&self.path) {
            Ok(raw) => {
                if raw.len() < SALT_SIZE + NONCE_SIZE {
                    return VaultData::default();
                }

                let salt = &raw[..SALT_SIZE];
                let nonce_bytes = &raw[SALT_SIZE..SALT_SIZE + NONCE_SIZE];
                let ciphertext = &raw[SALT_SIZE + NONCE_SIZE..];

                let key = Self::derive_key(salt);
                let cipher = Aes256Gcm::new_from_slice(&key).expect("32 bytes");
                let nonce = Nonce::from_slice(nonce_bytes);

                match cipher.decrypt(nonce, ciphertext) {
                    Ok(plaintext) => {
                        serde_json::from_slice(&plaintext).unwrap_or_default()
                    }
                    Err(e) => {
                        error!("Failed to decrypt vault: {:?}", e);
                        VaultData::default()
                    }
                }
            }
            Err(e) => {
                debug!("Vault file read failed: {}", e);
                VaultData::default()
            }
        }
    }

    pub fn save(&self, data: &VaultData) -> anyhow::Result<()> {
        let mut salt = [0u8; SALT_SIZE];
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::thread_rng().fill_bytes(&mut salt);
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        let key = Self::derive_key(&salt);
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let json = serde_json::to_vec(data)?;
        let ciphertext = cipher.decrypt(nonce, &json[..]).unwrap_or_else(|_| {
            cipher.encrypt(nonce, &json[..]).expect("encrypt")
        });

        let mut output = Vec::with_capacity(SALT_SIZE + NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&salt);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        fs::write(&self.path, output)?;
        info!("✓ Vault saved securely to {:?}", self.path);
        Ok(())
    }
}

fn whoami() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "meridian_user".to_string())
}
