//! Project preferences read from `leme.toml`.
//!
//! The file lives in the repository's main checkout, so every linked worktree
//! resolves the same preferences. See `docs/configuration.md`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};

/// Name of the preferences file in the root of the main checkout.
pub const FILE_NAME: &str = "leme.toml";

const DEFAULT_INTEGRATION_BRANCH: &str = "main";

/// The agent CLIs Leme can start for a feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        })
    }
}

/// How the dashboard writes the time of day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockFormat {
    /// `14:05`.
    #[default]
    #[serde(rename = "24h")]
    TwentyFourHour,
    /// `2:05 PM`.
    #[serde(rename = "12h")]
    TwelveHour,
}

impl fmt::Display for ClockFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TwentyFourHour => "24h",
            Self::TwelveHour => "12h",
        })
    }
}

/// The settings a project can choose, with defaults for any it leaves out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    /// Branch that feature branches start from and merge back into.
    #[serde(deserialize_with = "non_empty")]
    pub integration_branch: String,
    /// Agent started for a feature unless the user picks another.
    pub preferred_agent: Agent,
    /// How the dashboard shows the local time.
    pub clock_format: ClockFormat,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            integration_branch: DEFAULT_INTEGRATION_BRANCH.to_owned(),
            preferred_agent: Agent::default(),
            clock_format: ClockFormat::default(),
        }
    }
}

impl Preferences {
    /// Reads the preferences of the project whose main checkout is
    /// `repository`, using the defaults when the file does not exist.
    pub fn load(repository: &Path) -> Result<Self, ConfigError> {
        let path = repository.join(FILE_NAME);
        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text).map_err(|source| ConfigError::Invalid { path, source }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Unreadable { path, source }),
        }
    }

    /// Parses the contents of a preferences file.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

fn non_empty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(de::Error::custom("the branch name must not be empty"));
    }
    Ok(value)
}

/// Why a project's preferences could not be read.
#[derive(Debug)]
pub enum ConfigError {
    /// The file exists but could not be read.
    Unreadable { path: PathBuf, source: io::Error },
    /// The file is not valid TOML or holds an unsupported setting.
    Invalid {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
            // Parser errors quote the offending line and field.
            Self::Invalid { path, source } => {
                let source = source.to_string();
                write!(f, "invalid {}: {}", path.display(), source.trim_end())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            Self::Invalid { source, .. } => Some(source),
        }
    }
}
