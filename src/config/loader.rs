// Copyright 2026 Aravindh Sampath Kumar
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Configuration file loading and saving

use super::Config;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Load configuration from default location
pub fn load() -> Result<Config> {
    let path = Config::config_path()?;
    load_from(&path)
}

/// Load configuration from specific path
pub fn load_from(path: &Path) -> Result<Config> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config =
        toml::from_str(&contents).context("Failed to parse config file as TOML")?;

    Ok(config)
}

/// Save configuration to default location
pub fn save(config: &Config) -> Result<()> {
    let path = Config::config_path()?;
    save_to(config, &path)
}

/// Save configuration to specific path
pub fn save_to(config: &Config, path: &Path) -> Result<()> {
    // Ensure config directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    // Serialize config to TOML
    let contents = toml::to_string_pretty(config).context("Failed to serialize config to TOML")?;

    // Write to file
    fs::write(path, contents)
        .with_context(|| format!("Failed to write config file: {}", path.display()))?;

    // Set restrictive permissions (0600 - read/write for owner only)
    #[cfg(unix)]
    {
        let metadata = fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions).with_context(|| {
            format!(
                "Failed to set permissions on config file: {}",
                path.display()
            )
        })?;

        log::info!("Set config file permissions to 0600: {}", path.display());
    }

    #[cfg(not(unix))]
    {
        log::warn!("Cannot set file permissions on non-Unix system");
    }

    Ok(())
}

/// Check if config file exists
pub fn exists() -> bool {
    if let Ok(path) = Config::config_path() {
        path.exists()
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.toml");

        let original_config =
            Config::minimal("user@fastmail.com".to_string(), "testpassword".to_string());

        // Save
        save_to(&original_config, &config_path)?;

        // Load
        let loaded_config = load_from(&config_path)?;

        // Verify
        assert_eq!(
            loaded_config.server.username,
            original_config.server.username
        );
        assert_eq!(loaded_config.server.url, original_config.server.url);

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_file_permissions() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new()?;
        let config_path = temp_dir.path().join("config.toml");

        let config = Config::minimal("user@fastmail.com".to_string(), "testpassword".to_string());

        save_to(&config, &config_path)?;

        let metadata = fs::metadata(&config_path)?;
        let permissions = metadata.permissions();

        // Should be 0600
        assert_eq!(permissions.mode() & 0o777, 0o600);

        Ok(())
    }
}
