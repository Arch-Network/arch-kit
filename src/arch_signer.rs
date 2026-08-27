use std::{env, path::PathBuf, str::FromStr, time::Duration};

use arch_sdk::{
    Signature,
    arch_program::{pubkey::Pubkey, sanitized::ArchMessage},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use bitcoin::{Network, key::Keypair};
use serde_json::Value;

use crate::{
    error::{CliError, Result},
    keys::load_existing_key,
};

const COSIGNER_TIMEOUT: Duration = Duration::from_secs(35);

/// A transaction signer without exposing its key storage backend.
pub(crate) trait ArchSigner: Send + Sync {
    fn pubkey(&self) -> Pubkey;
    fn sign_message(&self, message: &ArchMessage) -> Result<Signature>;
}

/// A signer selected from a local key file or an arch-cosigner environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SignerSource {
    File(PathBuf),
    Cosigner(String),
}

impl SignerSource {
    pub(crate) fn resolve(
        &self,
        network: Network,
        intent: &'static str,
        label: &'static str,
    ) -> Result<Box<dyn ArchSigner>> {
        match self {
            Self::File(path) => Ok(Box::new(LocalFileSigner::from_file(path, network, label)?)),
            Self::Cosigner(prefix) => {
                Ok(Box::new(CosignerSigner::from_env(prefix, network, intent)?))
            }
        }
    }
}

impl FromStr for SignerSource {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        if let Some(path) = value.strip_prefix("file:") {
            return nonempty(path, "file signer path").map(|path| Self::File(path.into()));
        }
        if let Some(prefix) = value.strip_prefix("cosigner:") {
            let prefix = nonempty(prefix, "cosigner environment prefix")?;
            if !prefix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(
                    "cosigner environment prefix may contain only letters, numbers, and underscores"
                        .to_string(),
                );
            }
            return Ok(Self::Cosigner(prefix.to_ascii_uppercase()));
        }
        nonempty(value, "signer path").map(|path| Self::File(path.into()))
    }
}

fn nonempty<'a>(value: &'a str, label: &str) -> std::result::Result<&'a str, String> {
    if value.is_empty() {
        Err(format!("{label} must not be empty"))
    } else {
        Ok(value)
    }
}

pub(crate) struct LocalFileSigner {
    keypair: Keypair,
    pubkey: Pubkey,
    network: Network,
}

impl LocalFileSigner {
    fn from_file(path: &std::path::Path, network: Network, label: &'static str) -> Result<Self> {
        let (keypair, pubkey) = load_existing_key(path, label)?;
        Ok(Self {
            keypair,
            pubkey,
            network,
        })
    }
}

impl ArchSigner for LocalFileSigner {
    fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    fn sign_message(&self, message: &ArchMessage) -> Result<Signature> {
        Ok(Signature(arch_sdk::sign_message_bip322(
            &self.keypair,
            &message.hash(),
            self.network,
        )?))
    }
}

struct CosignerSigner {
    client: reqwest::blocking::Client,
    url: String,
    token: String,
    role: String,
    intent: &'static str,
    pubkey: Pubkey,
    network: Network,
}

impl CosignerSigner {
    fn from_env(prefix: &str, network: Network, intent: &'static str) -> Result<Self> {
        let read = |name: &str| {
            let variable = format!("{prefix}_{name}");
            env::var(&variable)
                .ok()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| CliError::Signer(format!("missing {variable}")))
        };
        let url = read("COSIGNER_URL")?.trim_end_matches('/').to_string();
        let token = read("COSIGNER_TOKEN")?;
        let role = read("COSIGNER_ROLE")?;
        let pubkey_hex = read("COSIGNER_PUBKEY")?;
        let pubkey_bytes: [u8; 32] = hex::decode(&pubkey_hex)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| {
                CliError::Signer(format!(
                    "{prefix}_COSIGNER_PUBKEY must contain exactly 64 hexadecimal characters"
                ))
            })?;
        let client = reqwest::blocking::Client::builder()
            .timeout(COSIGNER_TIMEOUT)
            .build()
            .map_err(|error| CliError::Signer(format!("cannot create cosigner client: {error}")))?;

        Ok(Self {
            client,
            url,
            token,
            role,
            intent,
            pubkey: Pubkey::from(pubkey_bytes),
            network,
        })
    }

    fn verify_response(&self, body: &Value, message: &ArchMessage) -> Result<Signature> {
        let signature: [u8; 64] = body["signature_hex"]
            .as_str()
            .and_then(|value| hex::decode(value).ok())
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| {
                CliError::Signer("cosigner signature_hex is missing or not 64 bytes".to_string())
            })?;
        let expected_pubkey = hex::encode(self.pubkey.serialize());
        if body["arch_account_pubkey"].as_str() != Some(expected_pubkey.as_str()) {
            return Err(CliError::Signer(format!(
                "cosigner returned public key {}, expected {expected_pubkey}",
                body["arch_account_pubkey"]
            )));
        }

        let digest = message.hash();
        if let Some(reported) = body["digest_hex"].as_str()
            && reported != hex::encode(&digest)
        {
            return Err(CliError::Signer(
                "cosigner returned a digest for a different message".to_string(),
            ));
        }
        arch_sdk::verify_message_bip322(
            &digest,
            self.pubkey.serialize(),
            signature,
            false,
            self.network,
        )
        .or_else(|_| {
            arch_sdk::verify_message_bip322(
                &digest,
                self.pubkey.serialize(),
                signature,
                true,
                self.network,
            )
        })
        .map_err(|error| {
            CliError::Signer(format!("cosigner response verification failed: {error}"))
        })?;
        Ok(Signature(signature))
    }
}

impl ArchSigner for CosignerSigner {
    fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    fn sign_message(&self, message: &ArchMessage) -> Result<Signature> {
        let response = self
            .client
            .post(format!("{}/v1/sign", self.url))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({
                "role": self.role,
                "intent_type": self.intent,
                "unsigned_message_b64": STANDARD.encode(message.serialize()),
            }))
            .send()
            .map_err(|error| CliError::Signer(format!("cosigner request failed: {error}")))?;
        let status = response.status();
        let body: Value = response
            .json()
            .map_err(|error| CliError::Signer(format!("invalid cosigner response: {error}")))?;
        if !status.is_success() {
            let detail = body["error"]
                .as_str()
                .or_else(|| body["halted"].as_str())
                .unwrap_or("no detail");
            return Err(CliError::Signer(format!(
                "cosigner returned HTTP {}: {detail}",
                status.as_u16()
            )));
        }
        self.verify_response(&body, message)
    }
}

#[cfg(test)]
mod tests {
    use arch_sdk::arch_program::{hash::Hash, sanitized::ArchMessage};
    use bitcoin::Network;

    use super::*;

    #[test]
    fn parses_file_and_cosigner_sources() {
        assert_eq!(
            "file:keys/owner.key".parse(),
            Ok(SignerSource::File("keys/owner.key".into()))
        );
        assert_eq!(
            "keys/owner.key".parse(),
            Ok(SignerSource::File("keys/owner.key".into()))
        );
        assert_eq!(
            "cosigner:deploy_authority".parse(),
            Ok(SignerSource::Cosigner("DEPLOY_AUTHORITY".to_string()))
        );
        assert!(SignerSource::from_str("cosigner:").is_err());
        assert!(SignerSource::from_str("cosigner:not-valid").is_err());
    }

    #[test]
    fn local_signer_produces_a_verifiable_signature() {
        let (keypair, pubkey, _) = arch_sdk::generate_new_keypair(Network::Testnet);
        let signer = LocalFileSigner {
            keypair,
            pubkey,
            network: Network::Testnet,
        };
        let message = ArchMessage::new(&[], Some(pubkey), Hash::from([7; 32]));
        let signature = signer.sign_message(&message).unwrap();

        arch_sdk::verify_message_bip322(
            &message.hash(),
            pubkey.serialize(),
            signature.0,
            false,
            Network::Testnet,
        )
        .or_else(|_| {
            arch_sdk::verify_message_bip322(
                &message.hash(),
                pubkey.serialize(),
                signature.0,
                true,
                Network::Testnet,
            )
        })
        .unwrap();
    }
}
