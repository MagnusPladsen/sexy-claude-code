use std::io::Write;

#[test]
fn test_config_file_roundtrip() {
    let toml_content = r#"
command = "claude --model opus"
theme = "nord"
fps = 60

[layout]
claude_pane_percent = 80
"#;

    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config_path = dir.path().join("config.toml");
    {
        let mut file = std::fs::File::create(&config_path).expect("Failed to create file");
        file.write_all(toml_content.as_bytes())
            .expect("Failed to write");
    }

    // Parse the TOML directly since Config::load is in the binary crate
    #[derive(serde::Deserialize)]
    #[serde(default)]
    struct Config {
        command: String,
        theme: String,
        fps: u32,
        layout: Layout,
    }

    #[derive(serde::Deserialize)]
    #[serde(default)]
    struct Layout {
        claude_pane_percent: u16,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                command: "claude".to_string(),
                theme: "catppuccin-mocha".to_string(),
                fps: 30,
                layout: Layout::default(),
            }
        }
    }

    impl Default for Layout {
        fn default() -> Self {
            Self {
                claude_pane_percent: 70,
            }
        }
    }

    let content = std::fs::read_to_string(&config_path).expect("Failed to read");
    let config: Config = toml::from_str(&content).expect("Failed to parse");

    assert_eq!(config.command, "claude --model opus");
    assert_eq!(config.theme, "nord");
    assert_eq!(config.fps, 60);
    assert_eq!(config.layout.claude_pane_percent, 80);
}

#[test]
fn test_empty_config_uses_defaults() {
    let toml_content = "";

    #[derive(serde::Deserialize)]
    #[serde(default)]
    struct Config {
        command: String,
        theme: String,
        fps: u32,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                command: "claude".to_string(),
                theme: "catppuccin-mocha".to_string(),
                fps: 30,
            }
        }
    }

    let config: Config = toml::from_str(toml_content).expect("Failed to parse empty config");
    assert_eq!(config.command, "claude");
    assert_eq!(config.theme, "catppuccin-mocha");
    assert_eq!(config.fps, 30);
}

#[test]
fn test_theme_file_parsing() {
    let theme_toml = r##"
name = "Test Theme"

[colors]
background = "#000000"
foreground = "#ffffff"
surface = "#111111"
overlay = "#222222"
primary = "#ff0000"
secondary = "#00ff00"
accent = "#0000ff"
success = "#00ff00"
warning = "#ffff00"
error = "#ff0000"
info = "#00ffff"
border = "#333333"
border_focused = "#ff00ff"
status_bg = "#444444"
status_fg = "#555555"
input_bg = "#666666"
input_fg = "#777777"
input_cursor = "#888888"
input_placeholder = "#999999"
"##;

    #[derive(serde::Deserialize)]
    struct ThemeFile {
        name: String,
        colors: Colors,
    }

    #[derive(serde::Deserialize)]
    struct Colors {
        background: String,
        foreground: String,
        primary: String,
    }

    let theme: ThemeFile = toml::from_str(theme_toml).expect("Failed to parse theme");
    assert_eq!(theme.name, "Test Theme");
    assert_eq!(theme.colors.background, "#000000");
    assert_eq!(theme.colors.foreground, "#ffffff");
    assert_eq!(theme.colors.primary, "#ff0000");
}
