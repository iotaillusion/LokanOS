use std::collections::BTreeMap;
use std::path::{Component, Path};

use async_trait::async_trait;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncReadExt;

use crate::state::Slot;

const SIGNATURE_PEM_LABEL: &str = "ED25519 SIGNATURE";
const DEFAULT_PUBLIC_KEY_LABEL: &str = "PUBLIC KEY";

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestComponent {
    pub name: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub build_sha: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub target_slot: Slot,
    pub components: Vec<ManifestComponent>,
}

#[derive(Debug, Clone)]
pub struct StageBundleMetadata {
    manifest: Manifest,
}

impl StageBundleMetadata {
    pub fn new(manifest: Manifest) -> Self {
        Self { manifest }
    }

    pub fn target_slot(&self) -> Slot {
        self.manifest.target_slot
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("bundle path {0} does not exist")]
    NotFound(String),
    #[error("bundle path {0} is not a directory")]
    NotDirectory(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse manifest: {0}")]
    Manifest(serde_json::Error),
    #[error("manifest must declare at least one component")]
    EmptyComponents,
    #[error("component path '{0}' is invalid")]
    InvalidComponentPath(String),
    #[error("component file missing: {0}")]
    MissingComponentFile(String),
    #[error("checksum entry missing for component: {0}")]
    MissingChecksumEntry(String),
    #[error("checksum mismatch for component {path}: expected {expected}, found {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("checksum file contains unexpected entry: {0}")]
    UnexpectedChecksumEntry(String),
    #[error("invalid checksum file format on line {line}: {details}")]
    InvalidChecksumFormat { line: usize, details: String },
    #[error("checksum file must be valid UTF-8: {0}")]
    InvalidChecksumEncoding(String),
    #[error("failed to parse signature: {0}")]
    InvalidSignature(String),
    #[error("signature file must be in PEM format with label '{SIGNATURE_PEM_LABEL}'")]
    UnexpectedSignatureLabel,
    #[error("signature length must be 64 bytes for Ed25519")]
    InvalidSignatureLength,
    #[error("bundle signature verification failed")]
    SignatureMismatch,
    #[error("failed to parse public key: {0}")]
    InvalidPublicKey(String),
    #[error("public key PEM must have label '{DEFAULT_PUBLIC_KEY_LABEL}'")]
    UnexpectedPublicKeyLabel,
}

#[async_trait]
pub trait BundleVerifier: Send + Sync {
    async fn verify(&self, bundle_path: &str) -> Result<StageBundleMetadata, BundleError>;
}

pub struct FilesystemBundleVerifier {
    verifying_key: VerifyingKey,
}

impl FilesystemBundleVerifier {
    pub fn from_public_key_pem(path: impl AsRef<Path>) -> Result<Self, BundleError> {
        let contents = std::fs::read(path)?;
        let pem =
            parse_pem(&contents).map_err(|err| BundleError::InvalidPublicKey(err.to_string()))?;
        if pem.tag() != DEFAULT_PUBLIC_KEY_LABEL {
            return Err(BundleError::UnexpectedPublicKeyLabel);
        }
        let verifying_key = VerifyingKey::from_public_key_der(pem.contents())
            .map_err(|err| BundleError::InvalidPublicKey(err.to_string()))?;
        Ok(Self { verifying_key })
    }
}

#[async_trait]
impl BundleVerifier for FilesystemBundleVerifier {
    async fn verify(&self, bundle_path: &str) -> Result<StageBundleMetadata, BundleError> {
        let root = Path::new(bundle_path);
        let metadata = fs::metadata(root).await;
        let metadata = match metadata {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(BundleError::NotFound(bundle_path.to_string()))
            }
            Err(err) => return Err(BundleError::Io(err)),
        };

        if !metadata.is_dir() {
            return Err(BundleError::NotDirectory(bundle_path.to_string()));
        }

        let manifest_path = root.join("manifest.json");
        let manifest_bytes = fs::read(&manifest_path).await?;
        let manifest: Manifest =
            serde_json::from_slice(&manifest_bytes).map_err(BundleError::Manifest)?;
        if manifest.components.is_empty() {
            return Err(BundleError::EmptyComponents);
        }

        let checksum_path = root.join("sig/sha256sum");
        let checksum_bytes = fs::read(&checksum_path).await?;
        let checksum_str = std::str::from_utf8(&checksum_bytes)
            .map_err(|err| BundleError::InvalidChecksumEncoding(err.to_string()))?;
        let mut checksum_entries = parse_sha256sum(checksum_str)?;

        for component in &manifest.components {
            validate_relative_path(&component.path)?;
            let component_path = root.join(&component.path);
            let component_metadata = fs::metadata(&component_path).await.map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    BundleError::MissingComponentFile(component.path.clone())
                } else {
                    BundleError::Io(err)
                }
            })?;
            if !component_metadata.is_file() {
                return Err(BundleError::MissingComponentFile(component.path.clone()));
            }

            let expected_checksum = component.sha256.to_lowercase();
            let checksum_entry = checksum_entries
                .remove(&component.path)
                .ok_or_else(|| BundleError::MissingChecksumEntry(component.path.clone()))?;
            if checksum_entry.to_lowercase() != expected_checksum {
                return Err(BundleError::ChecksumMismatch {
                    path: component.path.clone(),
                    expected: expected_checksum,
                    actual: checksum_entry,
                });
            }

            let actual_checksum = compute_sha256(&component_path).await?;
            if actual_checksum != expected_checksum {
                return Err(BundleError::ChecksumMismatch {
                    path: component.path.clone(),
                    expected: expected_checksum,
                    actual: actual_checksum,
                });
            }
        }

        if let Some((unexpected_path, _)) = checksum_entries.into_iter().next() {
            return Err(BundleError::UnexpectedChecksumEntry(unexpected_path));
        }

        let signature_path = root.join("sig/signature.pem");
        let signature_bytes = fs::read(&signature_path).await?;
        let signature_pem = parse_pem(&signature_bytes)
            .map_err(|err| BundleError::InvalidSignature(err.to_string()))?;
        if signature_pem.tag() != SIGNATURE_PEM_LABEL {
            return Err(BundleError::UnexpectedSignatureLabel);
        }

        let signature_array: [u8; 64] = signature_pem
            .contents()
            .try_into()
            .map_err(|_| BundleError::InvalidSignatureLength)?;
        let signature = Signature::from_bytes(&signature_array);

        self.verifying_key
            .verify(&checksum_bytes, &signature)
            .map_err(|_| BundleError::SignatureMismatch)?;

        Ok(StageBundleMetadata::new(manifest))
    }
}

fn parse_pem(bytes: &[u8]) -> Result<pem::Pem, pem::PemError> {
    pem::parse(bytes)
}

fn parse_sha256sum(contents: &str) -> Result<BTreeMap<String, String>, BundleError> {
    let mut map = BTreeMap::new();
    for (index, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(BundleError::InvalidChecksumFormat {
                line: index + 1,
                details: "expected '<sha256> <path>'".to_string(),
            });
        }

        let digest = parts[0].to_lowercase();
        if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(BundleError::InvalidChecksumFormat {
                line: index + 1,
                details: "invalid sha256 digest".to_string(),
            });
        }

        let path = parts[1].to_string();
        if map.insert(path.clone(), digest).is_some() {
            return Err(BundleError::InvalidChecksumFormat {
                line: index + 1,
                details: format!("duplicate entry for {path}"),
            });
        }
    }
    Ok(map)
}

fn validate_relative_path(path: &str) -> Result<(), BundleError> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(BundleError::InvalidComponentPath(path.to_string()));
    }

    for component in relative.components() {
        match component {
            Component::CurDir | Component::Normal(_) => {}
            _ => return Err(BundleError::InvalidComponentPath(path.to_string())),
        }
    }

    Ok(())
}

async fn compute_sha256(path: &Path) -> Result<String, BundleError> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8192];

    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}
