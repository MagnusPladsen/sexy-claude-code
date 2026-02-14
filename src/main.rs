mod app;
mod config;
mod keybindings;
mod pty;
mod terminal;
mod theme;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sexy-claude", about = "A beautiful terminal wrapper for Claude Code")]
#[command(version)]
struct Cli {
    /// Theme name (e.g., catppuccin-mocha, nord, dracula)
    #[arg(short, long)]
    theme: Option<String>,

    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Command to run (default: claude)
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = config::Config::load(cli.config.as_ref())
        .context("Failed to load configuration")?;

    // Determine theme: CLI flag > config > default
    let theme_name = cli.theme.as_deref().unwrap_or(&config.theme);
    let theme = theme::Theme::load(theme_name).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load theme '{}': {}. Using default.", theme_name, e);
        theme::Theme::default_theme()
    });

    // Determine command
    let command = if cli.command.is_empty() {
        config.command.clone()
    } else {
        cli.command.join(" ")
    };

    // Check that the command exists
    let program = command.split_whitespace().next().unwrap_or("claude");
    if which(program).is_none() {
        anyhow::bail!(
            "'{}' not found in PATH. Please install Claude Code first:\n  npm install -g @anthropic-ai/claude-code",
            program
        );
    }

    // Get terminal size
    let (cols, rows) = crossterm::terminal::size().context("Failed to get terminal size")?;
    if cols < 40 || rows < 10 {
        anyhow::bail!("Terminal too small ({}x{}). Need at least 40x10.", cols, rows);
    }

    // PTY gets the inner size (minus our chrome: 2 border rows + 1 status bar)
    let pty_rows = rows.saturating_sub(3);
    let pty_cols = cols.saturating_sub(2); // minus left/right borders

    // Extra environment for the child process
    let mut extra_env = HashMap::new();
    extra_env.insert("TERM_PROGRAM".to_string(), "sexy-claude".to_string());

    // Spawn PTY with claude
    let pty_process = pty::PtyProcess::spawn_with_env(&command, pty_cols, pty_rows, extra_env)
        .with_context(|| format!("Failed to spawn '{}'", command))?;

    // Initialize terminal
    let mut terminal = ratatui::init();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::SetTitle("sexy-claude")
    )?;

    // Run the app
    let mut app = app::App::new(config, theme, pty_process, rows, cols);
    let result = app.run(&mut terminal).await;

    // Cleanup — always restore terminal
    ratatui::restore();

    result
}

/// Simple which implementation
fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(program);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
    })
}
