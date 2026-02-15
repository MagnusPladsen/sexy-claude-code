use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub command: String,
    pub theme: String,
    pub fps: u32,
    pub layout: LayoutConfig,
    /// Claude model to use (e.g. "claude-sonnet-4-5-20250929").
    pub model: Option<String>,
    /// Effort level ("low", "medium", "high").
    pub effort: Option<String>,
    /// Maximum budget in USD per session.
    pub max_budget_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    pub claude_pane_percent: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            command: "claude".to_string(),
            theme: "catppuccin-mocha".to_string(),
            fps: 30,
            layout: LayoutConfig::default(),
            model: None,
            effort: None,
            max_budget_usd: None,
        }
    }
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            claude_pane_percent: 70,
        }
    }
}

impl Config {
    pub fn load(path: Option<&PathBuf>) -> Result<Self> {
        let config_path = match path {
            Some(p) => p.clone(),
            None => Self::default_path(),
        };

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

        let config: Config =
            toml::from_str(&content).with_context(|| "Failed to parse config TOML")?;

        config.validate()?;
        Ok(config)
    }

    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sexy-claude")
            .join("config.toml")
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.fps >= 1 && self.fps <= 120, "fps must be between 1 and 120");
        anyhow::ensure!(
            self.layout.claude_pane_percent >= 20 && self.layout.claude_pane_percent <= 100,
            "claude_pane_percent must be between 20 and 100"
        );
        Ok(())
    }
}

/// Save the selected theme name to the config file.
/// Preserves all other config values. Creates the file and parent dirs if needed.
pub fn save_theme(theme_name: &str, path: &std::path::Path) -> Result<()> {
    use std::collections::BTreeMap;

    // Read existing config as a generic TOML table (preserves unknown fields)
    let mut table: BTreeMap<String, toml::Value> = if path.exists() {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config at {}", path.display()))?;
        toml::from_str(&content).unwrap_or_default()
    } else {
        BTreeMap::new()
    };

    table.insert(
        "theme".to_string(),
        toml::Value::String(theme_name.to_string()),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(&table)
        .context("Failed to serialize config")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write config to {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.command, "claude");
        assert_eq!(config.theme, "catppuccin-mocha");
        assert_eq!(config.fps, 30);
        assert_eq!(config.layout.claude_pane_percent, 70);
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
            command = "claude"
            theme = "nord"
            fps = 60

            [layout]
            claude_pane_percent = 80
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.theme, "nord");
        assert_eq!(config.fps, 60);
        assert_eq!(config.layout.claude_pane_percent, 80);
    }

    #[test]
    fn test_partial_config() {
        let toml = r#"theme = "dracula""#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.theme, "dracula");
        assert_eq!(config.command, "claude");
        assert_eq!(config.fps, 30);
    }

    #[test]
    fn test_claude_options_config() {
        let toml = r#"
            model = "claude-sonnet-4-5-20250929"
            effort = "high"
            max_budget_usd = 5.0
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-5-20250929"));
        assert_eq!(config.effort.as_deref(), Some("high"));
        assert_eq!(config.max_budget_usd, Some(5.0));
    }

    #[test]
    fn test_claude_options_default_none() {
        let config = Config::default();
        assert!(config.model.is_none());
        assert!(config.effort.is_none());
        assert!(config.max_budget_usd.is_none());
    }

    #[test]
    fn test_validation_fps() {
        let config = Config {
            fps: 0,
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validation_pane_percent() {
        let config = Config {
            layout: LayoutConfig {
                claude_pane_percent: 10,
            },
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_load_nonexistent_returns_default() {
        let config = Config::load(Some(&PathBuf::from("/nonexistent/config.toml"))).unwrap();
        assert_eq!(config.command, "claude");
    }

    #[test]
    fn test_save_theme_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save_theme("nord", &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("theme = \"nord\""));
    }

    #[test]
    fn test_save_theme_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "command = \"custom-claude\"\ntheme = \"old\"\nfps = 60\n").unwrap();
        save_theme("dracula", &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("theme = \"dracula\""));
        assert!(content.contains("command = \"custom-claude\""));
        assert!(content.contains("fps = 60"));
    }

    #[test]
    fn test_save_theme_adds_to_existing_without_theme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "fps = 45\n").unwrap();
        save_theme("nord", &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("theme = \"nord\""));
        assert!(content.contains("fps = 45"));
    }
}
