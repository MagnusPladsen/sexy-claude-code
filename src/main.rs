mod app;
mod claude;
mod config;
mod cost;
mod diff;
mod git;
mod history;
mod keybindings;
mod pty;
mod terminal;
mod theme;
mod todo;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;
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

    /// Claude model to use (overrides config)
    #[arg(short, long)]
    model: Option<String>,

    /// Effort level: low, medium, high (overrides config)
    #[arg(short, long)]
    effort: Option<String>,

    /// Maximum budget in USD per session (overrides config)
    #[arg(long)]
    max_budget: Option<f64>,

    /// Path to MCP server config file (overrides config)
    #[arg(long)]
    mcp_config: Option<String>,

    /// Permission mode: default, plan, acceptEdits, bypassPermissions, delegate, dontAsk
    #[arg(long)]
    permission_mode: Option<String>,

    /// Bypass all permission checks (alias for --permission-mode bypassPermissions)
    #[arg(long)]
    dangerously_skip_permissions: bool,

    /// Tools to auto-allow (can be repeated, e.g. --allowed-tools Bash --allowed-tools Read)
    #[arg(long = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,

    /// Continue the most recent session
    #[arg(long = "continue")]
    continue_session: bool,

    /// Command to run (default: claude)
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut config = config::Config::load(cli.config.as_ref())
        .context("Failed to load configuration")?;

    // Apply CLI overrides to config
    if cli.mcp_config.is_some() {
        config.mcp_config = cli.mcp_config;
    }
    if cli.dangerously_skip_permissions {
        config.permission_mode = Some("bypassPermissions".to_string());
    } else if cli.permission_mode.is_some() {
        config.permission_mode = cli.permission_mode;
    }
    if cli.allowed_tools.is_some() {
        config.allowed_tools = cli.allowed_tools;
    }

    let theme_name = cli.theme.as_deref().unwrap_or(&config.theme);
    let theme = theme::Theme::load(theme_name).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load theme '{}': {}. Using default.", theme_name, e);
        theme::Theme::default_theme()
    });

    let command = if cli.command.is_empty() {
        config.command.clone()
    } else {
        cli.command.join(" ")
    };

    let program = command.split_whitespace().next().unwrap_or("claude");
    if which(program).is_none() {
        anyhow::bail!(
            "'{}' not found in PATH. Please install Claude Code first:\n  npm install -g @anthropic-ai/claude-code",
            program
        );
    }

    let (cols, rows) = crossterm::terminal::size().context("Failed to get terminal size")?;
    if cols < 40 || rows < 10 {
        anyhow::bail!("Terminal too small ({}x{}). Need at least 40x10.", cols, rows);
    }

    // Initialize terminal
    let mut terminal = ratatui::init();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::SetTitle("sexy-claude"),
        crossterm::event::EnableBracketedPaste
    )?;

    // Run the app — no more PTY setup needed, App handles process spawning
    let theme_name_owned = theme_name.to_string();
    let mut app = app::App::new(
        config,
        theme,
        theme_name_owned,
        command,
        cli.continue_session,
        cli.model,
        cli.effort,
        cli.max_budget,
    );
    let result = app.run(&mut terminal).await;

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableBracketedPaste
    );
    ratatui::restore();

    result
}

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
