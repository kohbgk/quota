pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod statusline;

use crate::model::ProviderStatus;

/// A token wrapper that never prints its value.
pub struct Secret(pub(crate) String);

impl Secret {
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Why a provider couldn't produce usage. Mapped to a user-facing [`ProviderStatus`].
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderError {
    NotInstalled,
    SignedOut(String),
    NeedsConsent,
    Expired,
    Waiting,
    Unsupported(String),
    RateLimited { retry_after_secs: Option<i64> },
    Network(String),
    Other(String),
}

impl ProviderError {
    pub fn to_status(&self, now_ms: i64, retry_at_ms: Option<i64>) -> ProviderStatus {
        match self {
            ProviderError::NotInstalled => ProviderStatus::NotInstalled,
            ProviderError::SignedOut(message) => ProviderStatus::SignedOut { message: message.clone() },
            ProviderError::NeedsConsent => ProviderStatus::NeedsConsent,
            ProviderError::Expired => ProviderStatus::Expired,
            ProviderError::Waiting => ProviderStatus::Waiting,
            ProviderError::Unsupported(message) => ProviderStatus::Unsupported { message: message.clone() },
            ProviderError::RateLimited { retry_after_secs } => ProviderStatus::RateLimited {
                retry_at: retry_at_ms.or(retry_after_secs.map(|s| now_ms + s * 1000)),
            },
            ProviderError::Network(message) | ProviderError::Other(message) => {
                ProviderStatus::Error { message: message.clone() }
            }
        }
    }
}

/// Looks up `name` in the directories listed in `PATH`.
pub fn find_on_path(names: &[&str]) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
