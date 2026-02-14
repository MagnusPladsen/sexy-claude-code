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

    fn default_path() -> PathBuf {
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
}
