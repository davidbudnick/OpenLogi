//! User configuration, persisted as TOML at the platform-standard config
//! path.
//!
//! Per-device state (button bindings, …) lives under the
//! [`Config::devices`] map, keyed by the HID++ identifier returned by
//! [`DeviceModelInfo::config_key`](crate::device::DeviceModelInfo::config_key)
//! — e.g. `"2b042"` for an MX Master 4. Schema migrations branch on
//! [`Config::schema_version`].

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::binding::{Action, ButtonId};
use crate::paths::{self, PathsError};

/// The schema version the current build produces. Bumped on breaking layout
/// changes; readers branch on the parsed value before consuming the rest of
/// the file.
pub const SCHEMA_VERSION: u32 = 1;

/// Top-level config document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            devices: BTreeMap::new(),
        }
    }
}

/// Settings scoped to a single physical device (keyed by HID++ model+ext).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub button_bindings: BTreeMap<ButtonId, Action>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not resolve config path")]
    Path(#[from] PathsError),
    #[error("could not read config at {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not parse config at {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not write config at {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not serialize config")]
    Serialize(#[from] toml::ser::Error),
    #[error("config at {path} has unsupported schema_version {found}")]
    UnsupportedSchemaVersion { path: PathBuf, found: u32 },
}

impl Config {
    /// Loads the config from the default user path, returning
    /// [`Config::default`] if the file does not exist yet.
    pub fn load_or_default() -> Result<Self, ConfigError> {
        Self::load_from_path(&paths::config_path()?)
    }

    /// Same as [`Self::load_or_default`] but reads from `path`. Used by tests
    /// to avoid touching the real user config.
    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => {
                let config: Self =
                    toml::from_str(&text).map_err(|source| ConfigError::Parse {
                        path: path.to_path_buf(),
                        source,
                    })?;
                if config.schema_version != SCHEMA_VERSION {
                    return Err(ConfigError::UnsupportedSchemaVersion {
                        path: path.to_path_buf(),
                        found: config.schema_version,
                    });
                }
                Ok(config)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the config atomically to the default user path: serialize to a
    /// sibling temp file, then rename over the target. On Unix the temp file
    /// is created with mode 0600.
    pub fn save_atomic(&self) -> Result<(), ConfigError> {
        self.save_to_path(&paths::config_path()?)
    }

    /// Same as [`Self::save_atomic`] but writes to `path`. Used by tests.
    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }
        let body = toml::to_string_pretty(self)?;
        write_atomic(path, body.as_bytes()).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Returns the bindings stored for `device_key`, or an empty map if the
    /// device has no committed bindings yet.
    #[must_use]
    pub fn bindings_for(&self, device_key: &str) -> BTreeMap<ButtonId, Action> {
        self.devices
            .get(device_key)
            .map(|d| d.button_bindings.clone())
            .unwrap_or_default()
    }

    /// Records `action` as the binding for `button` on `device_key`,
    /// creating the device entry if needed.
    pub fn set_binding(&mut self, device_key: &str, button: ButtonId, action: Action) {
        self.devices
            .entry(device_key.to_string())
            .or_default()
            .button_bindings
            .insert(button, action);
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("toml.tmp");
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)?;
            io::Write::write_all(&mut f, bytes)?;
            f.sync_all()?;
        }
        #[cfg(not(unix))]
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&tmp)?;
            io::Write::write_all(&mut f, bytes)?;
            f.sync_all()?;
        }
    }
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_and_read(config: &Config) -> Config {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        config.save_to_path(&path).expect("save");
        Config::load_from_path(&path).expect("load")
    }

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nonexistent.toml");
        let cfg = Config::load_from_path(&path).expect("load");
        assert_eq!(cfg.schema_version, SCHEMA_VERSION);
        assert!(cfg.devices.is_empty());
    }

    #[test]
    fn bindings_roundtrip_per_device() {
        let mut cfg = Config::default();
        cfg.set_binding("2b042", ButtonId::Back, Action::Copy);
        cfg.set_binding(
            "2b042",
            ButtonId::DpiToggle,
            Action::CustomShortcut("Toggle DPI".into()),
        );
        cfg.set_binding("4082d", ButtonId::Back, Action::Paste);

        let parsed = write_and_read(&cfg);

        // Per-device isolation.
        let a = parsed.bindings_for("2b042");
        assert_eq!(a.get(&ButtonId::Back), Some(&Action::Copy));
        assert_eq!(
            a.get(&ButtonId::DpiToggle),
            Some(&Action::CustomShortcut("Toggle DPI".into()))
        );

        let b = parsed.bindings_for("4082d");
        assert_eq!(b.get(&ButtonId::Back), Some(&Action::Paste));
        assert_eq!(b.len(), 1, "device b should only see its own bindings");

        // Unknown device returns empty map without panic.
        assert!(parsed.bindings_for("deadbeef").is_empty());
    }

    #[test]
    fn human_readable_toml_layout() {
        let mut cfg = Config::default();
        cfg.set_binding("2b042", ButtonId::Back, Action::BrowserBack);
        let body = toml::to_string_pretty(&cfg).expect("serialize");

        // The model id only contains [A-Za-z0-9_], so TOML emits it as a
        // bare-word table key (no surrounding quotes). The test asserts the
        // observable structure rather than locking in a specific quoting.
        assert!(body.contains("schema_version = 1"), "got: {body}");
        assert!(
            body.contains("[devices.2b042.button_bindings]"),
            "got: {body}"
        );
        assert!(body.contains("Back = \"BrowserBack\""), "got: {body}");
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        fs::write(&path, "schema_version = 99\n").expect("write");
        let err = Config::load_from_path(&path).expect_err("should fail");
        assert!(matches!(
            err,
            ConfigError::UnsupportedSchemaVersion { found: 99, .. }
        ));
    }

    #[test]
    fn empty_device_block_is_skipped_in_output() {
        // Inserting then clearing should not leave a [devices."x"] header
        // with no bindings under it (skip_serializing_if on button_bindings).
        let mut cfg = Config::default();
        cfg.set_binding("2b042", ButtonId::Back, Action::Copy);
        cfg.devices
            .get_mut("2b042")
            .expect("entry")
            .button_bindings
            .clear();
        let body = toml::to_string_pretty(&cfg).expect("serialize");
        assert!(
            !body.contains("Back"),
            "cleared bindings should not appear: {body}"
        );
    }
}
