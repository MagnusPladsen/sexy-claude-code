use anyhow::{Context, Result};
use ratatui::style::Color;
use serde::Deserialize;
use std::path::PathBuf;

const DEFAULT_THEME: &str = include_str!("../themes/catppuccin-mocha.toml");

#[derive(Debug, Deserialize)]
pub struct ThemeFile {
    pub name: String,
    pub colors: ThemeColors,
}

#[derive(Debug, Deserialize)]
pub struct ThemeColors {
    pub background: String,
    pub foreground: String,
    pub surface: String,
    pub overlay: String,

    pub primary: String,
    pub secondary: String,
    pub accent: String,

    pub success: String,
    pub warning: String,
    pub error: String,
    pub info: String,

    pub border: String,
    pub border_focused: String,

    pub status_bg: String,
    pub status_fg: String,

    pub input_bg: String,
    pub input_fg: String,
    pub input_cursor: String,
    pub input_placeholder: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub foreground: Color,
    pub surface: Color,
    pub overlay: Color,

    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,

    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    pub border: Color,
    pub border_focused: Color,

    pub status_bg: Color,
    pub status_fg: Color,

    pub input_bg: Color,
    pub input_fg: Color,
    pub input_cursor: Color,
    pub input_placeholder: Color,
}

impl Theme {
    pub fn load(name: &str) -> Result<Self> {
        // Try loading from themes directory next to the binary
        let theme_path = Self::theme_path(name);
        if theme_path.exists() {
            let content = std::fs::read_to_string(&theme_path)
                .with_context(|| format!("Failed to read theme {}", theme_path.display()))?;
            return Self::from_toml(&content);
        }

        // Try loading from user config directory
        let user_theme = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sexy-claude")
            .join("themes")
            .join(format!("{name}.toml"));
        if user_theme.exists() {
            let content = std::fs::read_to_string(&user_theme)
                .with_context(|| format!("Failed to read theme {}", user_theme.display()))?;
            return Self::from_toml(&content);
        }

        // Fall back to embedded default
        if name == "catppuccin-mocha" {
            return Self::from_toml(DEFAULT_THEME);
        }

        anyhow::bail!("Theme '{}' not found", name);
    }

    pub fn default_theme() -> Self {
        Self::from_toml(DEFAULT_THEME).expect("embedded default theme must be valid")
    }

    /// Discover all available theme names from bundled and user theme dirs.
    pub fn list_available() -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();

        // Bundled theme directories (exe-relative + cargo manifest)
        for dir in Self::theme_dirs() {
            Self::scan_theme_dir(&dir, &mut names);
        }

        // User themes: ~/.config/sexy-claude/themes/*.toml
        let user_themes = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sexy-claude")
            .join("themes");
        Self::scan_theme_dir(&user_themes, &mut names);

        // Always include the embedded default
        names.insert("catppuccin-mocha".to_string());

        names.into_iter().collect()
    }

    fn scan_theme_dir(dir: &std::path::Path, names: &mut std::collections::BTreeSet<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.insert(stem.to_string());
                    }
                }
            }
        }
    }

    /// Directories to search for bundled themes (in priority order).
    fn theme_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // Alongside the executable: ../themes/ (for installed binaries)
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                dirs.push(parent.join("../themes"));
            }
        }

        // Cargo project root (for cargo run / development)
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("themes"));

        dirs
    }

    fn theme_path(name: &str) -> PathBuf {
        let filename = format!("{name}.toml");
        for dir in Self::theme_dirs() {
            let path = dir.join(&filename);
            if path.exists() {
                return path;
            }
        }
        // Fallback (won't exist, but caller checks .exists())
        PathBuf::from("themes").join(filename)
    }

    /// Return the best-matching syntect theme name for syntax highlighting.
    pub fn syntax_theme_name(&self) -> &'static str {
        // Check for Catppuccin Mocha specifically
        if self.name.contains("Mocha") {
            return "base16-mocha.dark";
        }
        // Determine light vs dark by background luminance
        if let Color::Rgb(r, g, b) = self.background {
            let luminance = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
            if luminance > 128.0 {
                return "InspiredGitHub";
            }
        }
        "base16-ocean.dark"
    }

    fn from_toml(content: &str) -> Result<Self> {
        let file: ThemeFile =
            toml::from_str(content).with_context(|| "Failed to parse theme TOML")?;
        let c = &file.colors;

        Ok(Self {
            name: file.name,
            background: parse_hex(&c.background)?,
            foreground: parse_hex(&c.foreground)?,
            surface: parse_hex(&c.surface)?,
            overlay: parse_hex(&c.overlay)?,
            primary: parse_hex(&c.primary)?,
            secondary: parse_hex(&c.secondary)?,
            accent: parse_hex(&c.accent)?,
            success: parse_hex(&c.success)?,
            warning: parse_hex(&c.warning)?,
            error: parse_hex(&c.error)?,
            info: parse_hex(&c.info)?,
            border: parse_hex(&c.border)?,
            border_focused: parse_hex(&c.border_focused)?,
            status_bg: parse_hex(&c.status_bg)?,
            status_fg: parse_hex(&c.status_fg)?,
            input_bg: parse_hex(&c.input_bg)?,
            input_fg: parse_hex(&c.input_fg)?,
            input_cursor: parse_hex(&c.input_cursor)?,
            input_placeholder: parse_hex(&c.input_placeholder)?,
        })
    }
}

fn parse_hex(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches('#');
    anyhow::ensure!(hex.len() == 6, "Invalid hex color: #{hex}");

    let r = u8::from_str_radix(&hex[0..2], 16)
        .with_context(|| format!("Invalid red component in #{hex}"))?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .with_context(|| format!("Invalid green component in #{hex}"))?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .with_context(|| format!("Invalid blue component in #{hex}"))?;

    Ok(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_hex("#ff0000").unwrap(), Color::Rgb(255, 0, 0));
        assert_eq!(parse_hex("#00ff00").unwrap(), Color::Rgb(0, 255, 0));
        assert_eq!(parse_hex("#0000ff").unwrap(), Color::Rgb(0, 0, 255));
        assert_eq!(parse_hex("1e1e2e").unwrap(), Color::Rgb(30, 30, 46));
    }

    #[test]
    fn test_parse_hex_invalid() {
        assert!(parse_hex("#xyz").is_err());
        assert!(parse_hex("#12345").is_err());
    }

    #[test]
    fn test_default_theme() {
        let theme = Theme::default_theme();
        assert_eq!(theme.name, "Catppuccin Mocha");
        assert_eq!(theme.background, Color::Rgb(30, 30, 46));
    }

    #[test]
    fn test_load_default_theme() {
        let theme = Theme::load("catppuccin-mocha").unwrap();
        assert_eq!(theme.name, "Catppuccin Mocha");
    }

    #[test]
    fn test_load_nonexistent_theme() {
        assert!(Theme::load("nonexistent-theme").is_err());
    }

    #[test]
    fn test_list_available_includes_default() {
        let themes = Theme::list_available();
        assert!(themes.contains(&"catppuccin-mocha".to_string()));
    }

    #[test]
    fn test_list_available_sorted() {
        let themes = Theme::list_available();
        let mut sorted = themes.clone();
        sorted.sort();
        assert_eq!(themes, sorted);
    }

    #[test]
    fn test_all_bundled_themes_parse() {
        let theme_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        for entry in std::fs::read_dir(&theme_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let content = std::fs::read_to_string(&path).unwrap();
                let result = Theme::from_toml(&content);
                assert!(result.is_ok(), "Failed to parse theme {}: {:?}", path.display(), result.err());
            }
        }
    }
}
