use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::DefaultTerminal;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::claude::commands::{self, CustomCommand};
use crate::claude::conversation::Conversation;
use crate::claude::events::StreamEvent;
use crate::claude::process::{ClaudeProcess, SpawnOptions};
use crate::claude::sessions;
use crate::config::Config;
use crate::git::GitInfo;
use crate::theme::Theme;
use crate::todo::TodoTracker;
use crate::ui;
use crate::ui::header::HEADER_HEIGHT;
use crate::ui::input::InputEditor;
use crate::ui::overlay::{OverlayItem, OverlayState};
use crate::ui::toast::Toast;

enum Msg {
    ClaudeEvent(StreamEvent),
    ClaudeExited,
    Key(event::KeyEvent),
    Resize(u16, u16),
    Tick,
}

/// Actions for commands handled locally (not sent to Claude).
enum LocalAction {
    Clear,
}

enum AppMode {
    Normal,
    ActionMenu(OverlayState),
    ThemePicker(OverlayState),
    SessionPicker(OverlayState),
    TextViewer {
        title: String,
        lines: Vec<String>,
        scroll: usize,
    },
}

/// A single item in the slash command completion popup.
pub struct CompletionItem {
    pub name: String,
    pub description: String,
    pub score: i64,
}

/// Tracks slash command completion state.
pub struct CompletionState {
    pub matches: Vec<CompletionItem>,
    pub selected: usize,
}

impl CompletionState {
    fn new(matches: Vec<CompletionItem>) -> Self {
        Self {
            matches,
            selected: 0,
        }
    }

    fn move_up(&mut self) {
        if !self.matches.is_empty() {
            self.selected = self
                .selected
                .checked_sub(1)
                .unwrap_or(self.matches.len() - 1);
        }
    }

    fn move_down(&mut self) {
        if !self.matches.is_empty() {
            self.selected = (self.selected + 1) % self.matches.len();
        }
    }

    fn selected_command(&self) -> Option<&str> {
        self.matches.get(self.selected).map(|s| s.name.as_str())
    }
}

pub struct App {
    config: Config,
    theme: Theme,
    conversation: Conversation,
    claude: Option<ClaudeProcess>,
    input: InputEditor,
    should_quit: bool,
    frame_count: u64,
    mode: AppMode,
    theme_name: String,
    scroll_offset: usize,
    auto_scroll: bool,
    command: String,
    slash_commands: Vec<String>,
    custom_commands: Vec<CustomCommand>,
    completion: Option<CompletionState>,
    /// Tracks the last slash command sent, so we can show feedback for empty results.
    pending_slash_command: Option<String>,
    /// Brief notification shown after a slash command completes with no output.
    toast: Option<Toast>,
    /// Current session ID from Claude CLI system.init event.
    session_id: Option<String>,
    /// Main event sender, stored so we can forward events from resumed processes.
    event_tx: Option<mpsc::UnboundedSender<Msg>>,
    /// Cumulative token usage for this session.
    total_input_tokens: u64,
    total_output_tokens: u64,
    /// Whether to continue the most recent session on startup.
    continue_session: bool,
    /// Model override from CLI args.
    model_override: Option<String>,
    /// Effort override from CLI args.
    effort_override: Option<String>,
    /// Budget override from CLI args.
    budget_override: Option<f64>,
    /// Current git repo info (branch, dirty count).
    git_info: GitInfo,
    /// Frame counter at last git refresh (refresh every ~5s).
    git_last_refresh: u64,
    /// Tracks Claude's todo list from TodoWrite tool calls.
    todo_tracker: TodoTracker,
    /// Model name detected from the most recent MessageStart event.
    detected_model: Option<String>,
}

impl App {
    pub fn new(
        config: Config,
        theme: Theme,
        theme_name: String,
        command: String,
        continue_session: bool,
        model_override: Option<String>,
        effort_override: Option<String>,
        budget_override: Option<f64>,
    ) -> Self {
        Self {
            config,
            theme,
            conversation: Conversation::new(),
            claude: None,
            input: InputEditor::new(),
            should_quit: false,
            frame_count: 0,
            mode: AppMode::Normal,
            theme_name,
            scroll_offset: 0,
            auto_scroll: true,
            command,
            slash_commands: Vec::new(),
            custom_commands: commands::load_all_commands(),
            completion: None,
            pending_slash_command: None,
            toast: None,
            session_id: None,
            event_tx: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            continue_session,
            model_override,
            effort_override,
            budget_override,
            git_info: GitInfo::gather(),
            git_last_refresh: 0,
            todo_tracker: TodoTracker::new(),
            detected_model: None,
        }
    }

    /// Build spawn options from config + CLI overrides.
    fn build_spawn_options(&self) -> SpawnOptions {
        SpawnOptions {
            continue_session: self.continue_session,
            model: self
                .model_override
                .clone()
                .or_else(|| self.config.model.clone()),
            effort: self
                .effort_override
                .clone()
                .or_else(|| self.config.effort.clone()),
            max_budget_usd: self.budget_override.or(self.config.max_budget_usd),
            mcp_config: self.config.mcp_config.clone(),
            permission_mode: self.config.permission_mode.clone(),
            allowed_tools: self.config.allowed_tools.clone(),
            ..Default::default()
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
        self.event_tx = Some(tx.clone());

        // Spawn Claude process
        let options = self.build_spawn_options();
        let (claude_process, event_rx) =
            ClaudeProcess::spawn_with_options(&self.command, options)?;
        self.claude = Some(claude_process);
        Self::forward_claude_events(event_rx, tx.clone());

        // Spawn crossterm event reader task
        let tx_event = tx.clone();
        std::thread::spawn(move || {
            event_reader_loop(tx_event);
        });

        // Spawn tick task
        let tick_ms = 1000 / self.config.fps as u64;
        let tx_tick = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(tick_ms));
            loop {
                interval.tick().await;
                if tx_tick.send(Msg::Tick).is_err() {
                    break;
                }
            }
        });

        // Initial render
        self.view(terminal)?;

        // Event loop
        while let Some(msg) = rx.recv().await {
            self.update(msg).await?;
            if self.should_quit {
                break;
            }
            self.view(terminal)?;
        }

        // Cleanup
        if let Some(ref mut claude) = self.claude {
            let _ = claude.kill().await;
        }

        Ok(())
    }

    /// Forward Claude events from a process receiver to the main event channel.
    fn forward_claude_events(
        mut event_rx: mpsc::UnboundedReceiver<StreamEvent>,
        tx: mpsc::UnboundedSender<Msg>,
    ) {
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if tx.send(Msg::ClaudeEvent(event)).is_err() {
                    break;
                }
            }
            let _ = tx.send(Msg::ClaudeExited);
        });
    }

    /// Resume a session: kill current process, reset state, spawn with --resume.
    async fn resume_session(&mut self, session_id: &str) -> Result<()> {
        // Kill the current process
        if let Some(ref mut claude) = self.claude {
            let _ = claude.kill().await;
        }
        self.claude = None;

        // Reset conversation state
        self.conversation = Conversation::new();
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.slash_commands.clear();
        self.session_id = None;

        // Spawn new process with --resume + config options
        let mut options = self.build_spawn_options();
        options.resume_session_id = Some(session_id.to_string());
        options.continue_session = false;
        let (claude_process, event_rx) =
            ClaudeProcess::spawn_with_options(&self.command, options)?;
        self.claude = Some(claude_process);

        // Forward events from the new process
        if let Some(ref tx) = self.event_tx {
            Self::forward_claude_events(event_rx, tx.clone());
        }

        self.toast = Some(Toast::new("Resuming session...".to_string()));

        Ok(())
    }

    /// Continue the most recent session using --continue.
    async fn continue_last_session(&mut self) -> Result<()> {
        if let Some(ref mut claude) = self.claude {
            let _ = claude.kill().await;
        }
        self.claude = None;
        self.conversation = Conversation::new();
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.slash_commands.clear();
        self.session_id = None;

        let (claude_process, event_rx) =
            ClaudeProcess::spawn_with_continue(&self.command)?;
        self.claude = Some(claude_process);

        if let Some(ref tx) = self.event_tx {
            Self::forward_claude_events(event_rx, tx.clone());
        }

        self.toast = Some(Toast::new("Continuing last session...".to_string()));

        Ok(())
    }

    async fn update(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::ClaudeEvent(event) => {
                // Extract slash commands and session ID from SystemInit
                if let StreamEvent::SystemInit {
                    ref slash_commands,
                    ref session_id,
                } = event
                {
                    self.slash_commands = slash_commands.clone();
                    self.session_id = session_id.clone();
                }

                // Show toast for empty slash command results, clear tracking
                if let StreamEvent::Result { ref text, is_error, ref permission_denials } = event {
                    if !permission_denials.is_empty() {
                        let denied: Vec<&str> = permission_denials
                            .iter()
                            .map(|d| d.tool_name.as_str())
                            .collect();
                        self.toast = Some(Toast::new(format!(
                            "Permission denied: {}",
                            denied.join(", ")
                        )));
                    } else if text.is_empty() && !is_error {
                        if let Some(cmd) = self.pending_slash_command.as_ref() {
                            self.toast = Some(Toast::new(format!("Ran {cmd}")));
                        }
                    }
                    self.pending_slash_command.take();
                }

                // Capture model name and clear pending command on new message
                if let StreamEvent::MessageStart { ref model, .. } = event {
                    self.pending_slash_command = None;
                    if self.detected_model.is_none() || !model.is_empty() {
                        self.detected_model = Some(model.clone());
                    }
                }

                // Show toast for hook lifecycle events
                if let StreamEvent::SystemHook { ref subtype, .. } = event {
                    match subtype.as_str() {
                        "hook_started" => {
                            self.toast = Some(Toast::new("Hook running...".to_string()));
                        }
                        "hook_completed" => {
                            self.toast = Some(Toast::new("Hook completed".to_string()));
                        }
                        _ => {}
                    }
                }

                // Accumulate token usage
                match &event {
                    StreamEvent::MessageStart {
                        usage: Some(u), ..
                    } => {
                        self.total_input_tokens += u.input_tokens;
                        self.total_output_tokens += u.output_tokens;
                    }
                    StreamEvent::MessageDelta {
                        usage: Some(u), ..
                    } => {
                        self.total_output_tokens += u.output_tokens;
                    }
                    _ => {}
                }

                // Update todo tracker when a TodoWrite tool_use block completes
                if let StreamEvent::ContentBlockStop { index } = &event {
                    if let Some(msg) = self.conversation.messages.last() {
                        if let Some(crate::claude::conversation::ContentBlock::ToolUse {
                            name, input, ..
                        }) = msg.content.get(*index)
                        {
                            if name == "TodoWrite" {
                                self.todo_tracker.apply_todo_write(input);
                            }
                        }
                    }
                }

                self.conversation.apply_event(&event);
                if self.auto_scroll {
                    self.scroll_to_bottom();
                }
            }
            Msg::ClaudeExited => {
                // Claude process ended
            }
            Msg::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }
                self.handle_key(key).await?;
            }
            Msg::Resize(_width, _height) => {
                if self.auto_scroll {
                    self.scroll_to_bottom();
                }
            }
            Msg::Tick => {
                self.frame_count = self.frame_count.wrapping_add(1);
                // Expire toast notifications
                if self.toast.as_ref().is_some_and(|t| t.is_expired()) {
                    self.toast = None;
                }
                // Refresh git info every ~5 seconds
                let refresh_interval = (self.config.fps as u64) * 5;
                if self.frame_count - self.git_last_refresh >= refresh_interval {
                    self.git_info = GitInfo::gather();
                    self.git_last_refresh = self.frame_count;
                }
            }
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: event::KeyEvent) -> Result<()> {
        match &self.mode {
            AppMode::Normal => self.handle_key_normal(key).await,
            AppMode::ActionMenu(_)
            | AppMode::ThemePicker(_)
            | AppMode::SessionPicker(_) => self.handle_key_overlay(key).await,
            AppMode::TextViewer { .. } => self.handle_key_text_viewer(key),
        }
    }

    async fn handle_key_normal(&mut self, key: event::KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        if ctrl && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('k') {
            self.open_action_menu();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('t') {
            self.open_theme_picker();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('r') {
            self.open_session_picker();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('i') {
            self.open_instructions_viewer();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('f') {
            self.open_file_context_panel();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('d') {
            self.open_diff_viewer();
            return Ok(());
        }

        // Scrolling
        match key.code {
            KeyCode::PageUp => {
                self.auto_scroll = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                return Ok(());
            }
            KeyCode::PageDown => {
                self.scroll_offset += 10;
                self.clamp_scroll();
                return Ok(());
            }
            _ => {}
        }

        // Completion navigation (when popup is visible)
        if self.completion.is_some() {
            match key.code {
                KeyCode::Tab | KeyCode::Enter if !shift => {
                    // Accept selected completion
                    if let Some(ref state) = self.completion {
                        if let Some(cmd) = state.selected_command() {
                            let full = format!("/{cmd}");
                            self.input.set_content(&full);
                        }
                    }
                    self.completion = None;
                    return Ok(());
                }
                KeyCode::Esc => {
                    self.completion = None;
                    return Ok(());
                }
                KeyCode::Up => {
                    if let Some(ref mut state) = self.completion {
                        state.move_up();
                    }
                    return Ok(());
                }
                KeyCode::Down => {
                    if let Some(ref mut state) = self.completion {
                        state.move_down();
                    }
                    return Ok(());
                }
                _ => {
                    // Fall through to normal input handling, then update completions
                }
            }
        }

        // Input handling
        match key.code {
            KeyCode::Enter if !shift => {
                if !self.input.is_empty() && !self.conversation.is_streaming() {
                    let text = self.input.take_content();

                    if let Some(action) = self.handle_local_command(&text) {
                        // Command handled locally
                        match action {
                            LocalAction::Clear => {
                                self.conversation = Conversation::new();
                                self.scroll_offset = 0;
                                self.auto_scroll = true;
                            }
                        }
                    } else if let Some(prompt) = self.resolve_custom_command(&text) {
                        // Custom command — substitute args and send as user message
                        self.conversation.push_user_message(prompt.clone());
                        self.auto_scroll = true;
                        self.scroll_to_bottom();
                        if let Some(ref mut claude) = self.claude {
                            claude.send_message(&prompt).await?;
                        }
                    } else if text.starts_with('/') {
                        // Slash command — send to Claude but don't add as user message
                        self.pending_slash_command = Some(text.clone());
                        self.auto_scroll = true;
                        self.scroll_to_bottom();
                        if let Some(ref mut claude) = self.claude {
                            claude.send_message(&text).await?;
                        }
                    } else {
                        // Normal user message — expand @file mentions before sending
                        self.conversation.push_user_message(text.clone());
                        self.auto_scroll = true;
                        self.scroll_to_bottom();
                        let expanded = expand_file_mentions(&text);
                        if let Some(ref mut claude) = self.claude {
                            claude.send_message(&expanded).await?;
                        }
                    }
                }
            }
            KeyCode::Enter if shift => {
                self.input.insert_newline();
            }
            KeyCode::Char(c) if !ctrl => {
                self.input.insert_char(c);
            }
            KeyCode::Backspace => {
                self.input.backspace();
            }
            KeyCode::Delete => {
                self.input.delete();
            }
            KeyCode::Left => {
                self.input.move_left();
            }
            KeyCode::Right => {
                self.input.move_right();
            }
            KeyCode::Home => {
                self.input.move_home();
            }
            KeyCode::End => {
                self.input.move_end();
            }
            _ => {}
        }

        // Update slash command completions based on current input
        self.update_completions();

        Ok(())
    }

    async fn handle_key_overlay(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
            }
            KeyCode::Enter => {
                self.confirm_overlay().await?;
            }
            KeyCode::Up => {
                self.overlay_state_mut(|s| s.move_up());
                self.preview_theme();
            }
            KeyCode::Down => {
                self.overlay_state_mut(|s| s.move_down());
                self.preview_theme();
            }
            KeyCode::Backspace => {
                self.overlay_state_mut(|s| s.backspace());
            }
            KeyCode::Char(c) => {
                self.overlay_state_mut(|s| s.type_char(c));
            }
            _ => {}
        }
        Ok(())
    }

    /// Apply a mutation to the current overlay state (if any).
    fn overlay_state_mut(&mut self, f: impl FnOnce(&mut OverlayState)) {
        match self.mode {
            AppMode::ActionMenu(ref mut state)
            | AppMode::ThemePicker(ref mut state)
            | AppMode::SessionPicker(ref mut state) => f(state),
            AppMode::Normal | AppMode::TextViewer { .. } => {}
        }
    }

    /// Build a unified list of completion items from slash commands and custom commands.
    fn all_completion_items(&self) -> Vec<CompletionItem> {
        let mut items: Vec<CompletionItem> = self
            .slash_commands
            .iter()
            .map(|cmd| CompletionItem {
                name: cmd.clone(),
                description: String::new(),
                score: 0,
            })
            .collect();

        for cmd in &self.custom_commands {
            // Skip if a built-in slash command already has this name
            if items.iter().any(|i| i.name == cmd.name) {
                continue;
            }
            items.push(CompletionItem {
                name: cmd.name.clone(),
                description: cmd.description.clone(),
                score: 0,
            });
        }

        items
    }

    /// Update slash command completions based on current input text using fuzzy matching.
    fn update_completions(&mut self) {
        let content = self.input.content();
        if !content.starts_with('/') || content.contains(' ') || content.contains('\n') {
            self.completion = None;
            return;
        }

        let query = &content[1..]; // strip the leading '/'
        let all_items = self.all_completion_items();

        if query.is_empty() {
            // Show all commands when just "/" is typed
            if all_items.is_empty() {
                self.completion = None;
            } else {
                self.completion = Some(CompletionState::new(all_items));
            }
            return;
        }

        let matcher = SkimMatcherV2::default();
        let mut matches: Vec<CompletionItem> = all_items
            .into_iter()
            .filter_map(|item| {
                matcher
                    .fuzzy_match(&item.name, query)
                    .map(|score| CompletionItem { score, ..item })
            })
            .collect();

        // Sort by score descending (best match first)
        matches.sort_by(|a, b| b.score.cmp(&a.score));

        if matches.is_empty() {
            self.completion = None;
        } else {
            // Preserve selection index if possible
            let prev_selected = self
                .completion
                .as_ref()
                .map(|s| s.selected)
                .unwrap_or(0);
            let mut state = CompletionState::new(matches);
            state.selected = prev_selected.min(state.matches.len().saturating_sub(1));
            self.completion = Some(state);
        }
    }

    /// Check if the input matches a custom command. Returns the rendered prompt if so.
    ///
    /// Format: `/command-name optional arguments here`
    fn resolve_custom_command(&self, text: &str) -> Option<String> {
        if !text.starts_with('/') {
            return None;
        }

        let without_slash = &text[1..];
        let (cmd_name, args) = match without_slash.find(' ') {
            Some(pos) => (&without_slash[..pos], without_slash[pos + 1..].trim()),
            None => (without_slash, ""),
        };

        self.custom_commands
            .iter()
            .find(|c| c.name == cmd_name)
            .map(|c| c.render(args))
    }

    /// Check if the input is a command that should be handled locally.
    fn handle_local_command(&self, text: &str) -> Option<LocalAction> {
        let trimmed = text.trim();
        match trimmed {
            "/clear" => Some(LocalAction::Clear),
            _ => None,
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = usize::MAX;
    }

    fn clamp_scroll(&mut self) {
        let total = ui::claude_pane::total_lines(&self.conversation, 80, &self.theme);
        let max_scroll = total.saturating_sub(10);
        if self.scroll_offset >= max_scroll {
            self.scroll_offset = max_scroll;
            self.auto_scroll = true;
        }
    }

    fn open_theme_picker(&mut self) {
        let themes = crate::theme::Theme::list_available();
        let items: Vec<OverlayItem> = themes
            .iter()
            .map(|name| {
                let display = crate::theme::Theme::load(name)
                    .map(|t| t.name)
                    .unwrap_or_else(|_| name.clone());
                OverlayItem {
                    label: display,
                    value: name.clone(),
                    hint: String::new(),
                }
            })
            .collect();

        let current_idx = items
            .iter()
            .position(|i| i.value == self.theme_name)
            .unwrap_or(0);
        let mut state = OverlayState::new(items, Some(self.theme_name.clone()));
        state.selected = current_idx;
        self.mode = AppMode::ThemePicker(state);
    }

    fn open_action_menu(&mut self) {
        let items = vec![
            OverlayItem {
                label: "Continue Last Session".to_string(),
                value: "continue".to_string(),
                hint: String::new(),
            },
            OverlayItem {
                label: "Resume Session".to_string(),
                value: "resume".to_string(),
                hint: "Ctrl+R".to_string(),
            },
            OverlayItem {
                label: "Compact Context".to_string(),
                value: "compact".to_string(),
                hint: String::new(),
            },
            OverlayItem {
                label: "Switch Theme".to_string(),
                value: "theme".to_string(),
                hint: "Ctrl+T".to_string(),
            },
            OverlayItem {
                label: "Quit".to_string(),
                value: "quit".to_string(),
                hint: "Ctrl+Q".to_string(),
            },
        ];
        self.mode = AppMode::ActionMenu(OverlayState::new(items, None));
    }

    fn open_session_picker(&mut self) {
        let all_sessions = sessions::discover_sessions();
        let items: Vec<OverlayItem> = all_sessions
            .into_iter()
            .take(50)
            .map(|s| {
                let label = if s.preview.is_empty() {
                    format!("{} ({})", s.project_path, s.age_string())
                } else {
                    format!("{} — {}", s.age_string(), s.preview)
                };
                OverlayItem {
                    label,
                    value: s.session_id,
                    hint: s.project_path,
                }
            })
            .collect();

        if items.is_empty() {
            self.toast = Some(Toast::new("No sessions found".to_string()));
            return;
        }

        self.mode = AppMode::SessionPicker(OverlayState::new(items, None));
    }

    fn preview_theme(&mut self) {
        if let AppMode::ThemePicker(ref state) = self.mode {
            if let Some(value) = state.selected_value() {
                if let Ok(new_theme) = crate::theme::Theme::load(&value) {
                    self.theme = new_theme;
                }
            }
        }
    }

    fn close_overlay(&mut self) {
        if let AppMode::ThemePicker(ref state) = self.mode {
            if let Some(ref original) = state.original_theme {
                if let Ok(theme) = crate::theme::Theme::load(original) {
                    self.theme = theme;
                }
            }
        }
        self.mode = AppMode::Normal;
    }

    async fn confirm_overlay(&mut self) -> Result<()> {
        let mode = std::mem::replace(&mut self.mode, AppMode::Normal);

        match mode {
            AppMode::ThemePicker(state) => {
                if let Some(value) = state.selected_value() {
                    if let Ok(new_theme) = crate::theme::Theme::load(&value) {
                        self.theme = new_theme;
                        self.theme_name = value.clone();
                        let config_path = crate::config::Config::default_path();
                        let _ = crate::config::save_theme(&value, &config_path);
                    }
                }
            }
            AppMode::ActionMenu(state) => {
                if let Some(value) = state.selected_value() {
                    match value.as_str() {
                        "continue" => self.continue_last_session().await?,
                        "resume" => self.open_session_picker(),
                        "compact" => {
                            self.pending_slash_command = Some("/compact".to_string());
                            if let Some(ref mut claude) = self.claude {
                                claude.send_message("/compact").await?;
                            }
                            self.toast = Some(Toast::new("Compacting context...".to_string()));
                        }
                        "theme" => self.open_theme_picker(),
                        "quit" => self.should_quit = true,
                        _ => {}
                    }
                }
            }
            AppMode::SessionPicker(state) => {
                if let Some(session_id) = state.selected_value() {
                    self.resume_session(&session_id).await?;
                }
            }
            AppMode::Normal | AppMode::TextViewer { .. } => {}
        }
        Ok(())
    }

    fn open_instructions_viewer(&mut self) {
        // Search for CLAUDE.md in current directory and parents
        let mut dir = std::env::current_dir().ok();
        let mut content = None;
        while let Some(ref d) = dir {
            let path = d.join("CLAUDE.md");
            if path.exists() {
                content = std::fs::read_to_string(&path).ok();
                break;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }

        let text = match content {
            Some(c) => c,
            None => {
                self.toast = Some(Toast::new("No CLAUDE.md found".to_string()));
                return;
            }
        };

        let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
        self.mode = AppMode::TextViewer {
            title: "CLAUDE.md".to_string(),
            lines,
            scroll: 0,
        };
    }

    fn open_diff_viewer(&mut self) {
        use crate::claude::conversation::ContentBlock;

        // Collect all Edit tool diffs from the conversation
        let mut diff_text = String::new();
        for msg in &self.conversation.messages {
            for block in &msg.content {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    if name == "Edit" {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
                            let file_path = value
                                .get("file_path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let old = value
                                .get("old_string")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let new = value
                                .get("new_string")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            if !old.is_empty() || !new.is_empty() {
                                diff_text.push_str(&format!("--- {file_path}\n+++ {file_path}\n"));
                                let ops = crate::diff::diff_lines(old, new);
                                diff_text.push_str(&crate::diff::format_unified(&ops));
                                diff_text.push('\n');
                            }
                        }
                    } else if name == "Write" {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
                            let file_path = value
                                .get("file_path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let content = value
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let line_count = content.lines().count();
                            diff_text
                                .push_str(&format!("+++ {file_path} (new file, {line_count} lines)\n\n"));
                        }
                    }
                }
            }
        }

        if diff_text.is_empty() {
            self.toast = Some(Toast::new("No file changes in this session".to_string()));
            return;
        }

        let lines: Vec<String> = diff_text.lines().map(|l| l.to_string()).collect();
        self.mode = AppMode::TextViewer {
            title: "Session Diffs".to_string(),
            lines,
            scroll: 0,
        };
    }

    fn open_file_context_panel(&mut self) {
        use crate::claude::conversation::ContentBlock;
        use std::collections::BTreeMap;

        // Collect file operations from conversation tool uses
        let file_tools = ["Read", "Write", "Edit", "Glob", "Grep"];
        let mut file_ops: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for msg in &self.conversation.messages {
            for block in &msg.content {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    if !file_tools.contains(&name.as_str()) {
                        continue;
                    }
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
                        let path = value
                            .get("file_path")
                            .or_else(|| value.get("path"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if !path.is_empty() {
                            file_ops
                                .entry(path.to_string())
                                .or_default()
                                .push(name.clone());
                        }
                    }
                }
            }
        }

        if file_ops.is_empty() {
            self.toast = Some(Toast::new("No file operations in this session".to_string()));
            return;
        }

        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("{} files accessed", file_ops.len()));
        lines.push(String::new());

        for (path, ops) in &file_ops {
            let summary: Vec<&str> = ops.iter().map(|s| s.as_str()).collect();
            lines.push(format!("  {} [{}]", path, summary.join(", ")));
        }

        self.mode = AppMode::TextViewer {
            title: "File Context".to_string(),
            lines,
            scroll: 0,
        };
    }

    fn handle_key_text_viewer(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let AppMode::TextViewer { ref mut scroll, .. } = self.mode {
                    *scroll = scroll.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let AppMode::TextViewer { ref mut scroll, .. } = self.mode {
                    *scroll += 1;
                }
            }
            KeyCode::PageUp => {
                if let AppMode::TextViewer { ref mut scroll, .. } = self.mode {
                    *scroll = scroll.saturating_sub(20);
                }
            }
            KeyCode::PageDown => {
                if let AppMode::TextViewer { ref mut scroll, .. } = self.mode {
                    *scroll += 20;
                }
            }
            KeyCode::Home => {
                if let AppMode::TextViewer { ref mut scroll, .. } = self.mode {
                    *scroll = 0;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn view(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let theme = &self.theme;
        let frame_count = self.frame_count;
        let overlay = match &self.mode {
            AppMode::ActionMenu(state) => Some(("Actions", state)),
            AppMode::ThemePicker(state) => Some(("Select Theme", state)),
            AppMode::SessionPicker(state) => Some(("Resume Session", state)),
            AppMode::Normal | AppMode::TextViewer { .. } => None,
        };

        // Clamp scroll before rendering
        let term_size = terminal.size()?;
        let visible_height = term_size.height.saturating_sub(HEADER_HEIGHT + 4) as usize;
        let total_conv_lines = ui::claude_pane::total_lines(
            &self.conversation,
            term_size.width.saturating_sub(4) as usize,
            &self.theme,
        );
        if self.auto_scroll || self.scroll_offset > total_conv_lines {
            self.scroll_offset = total_conv_lines.saturating_sub(visible_height);
        }

        let conversation = &self.conversation;
        let input = &self.input;
        let scroll_offset = self.scroll_offset;
        let is_streaming = self.conversation.is_streaming();
        let completion = self.completion.as_ref();
        let toast = self.toast.as_ref();
        let token_usage = (self.total_input_tokens, self.total_output_tokens);
        let git_info = &self.git_info;
        let todo_summary = self.todo_tracker.summary();
        let model_name = self.detected_model.as_deref()
            .or(self.model_override.as_deref())
            .or(self.config.model.as_deref());
        let permission_mode = self.config.permission_mode.as_deref();
        let text_viewer = match &self.mode {
            AppMode::TextViewer {
                title,
                lines,
                scroll,
            } => Some((title.as_str(), lines.as_slice(), *scroll)),
            _ => None,
        };

        terminal.draw(|frame| {
            ui::render(
                frame,
                conversation,
                input,
                theme,
                frame_count,
                scroll_offset,
                is_streaming,
                completion,
                toast,
                token_usage,
                git_info,
                todo_summary.as_deref(),
                model_name,
                permission_mode,
            );
            if let Some((title, state)) = overlay {
                ui::render_overlay(frame, title, state, theme);
            }
            if let Some((title, lines, scroll)) = text_viewer {
                ui::render_text_viewer(frame, title, lines, scroll, theme);
            }
        })?;

        Ok(())
    }
}

/// Expand `@path/to/file` mentions in user input by reading the referenced files
/// and prepending their content. The original mention remains in the text so Claude
/// knows which file was referenced.
///
/// Rules:
/// - `@` must be preceded by whitespace or be at the start of the text
/// - The path extends until the next whitespace or end of text
/// - Only existing files are expanded; non-existent paths are left as-is
fn expand_file_mentions(text: &str) -> String {
    use std::path::Path;

    // Quick bail — no @ means nothing to expand
    if !text.contains('@') {
        return text.to_string();
    }

    let mut file_contents: Vec<(String, String)> = Vec::new();

    // Find @mentions: look for @ preceded by whitespace or at start
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            let at_start = i == 0;
            let after_space = i > 0 && chars[i - 1].is_whitespace();
            if at_start || after_space {
                // Extract the path: everything until next whitespace
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && !chars[end].is_whitespace() {
                    end += 1;
                }
                if end > start {
                    let path_str: String = chars[start..end].iter().collect();
                    let path = Path::new(&path_str);
                    if path.exists() && path.is_file() {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            // Limit to 100KB to avoid massive context injection
                            let truncated = if content.len() > 100_000 {
                                format!("{}...\n[truncated, file is {} bytes]", &content[..100_000], content.len())
                            } else {
                                content
                            };
                            file_contents.push((path_str, truncated));
                        }
                    }
                }
            }
        }
        i += 1;
    }

    if file_contents.is_empty() {
        return text.to_string();
    }

    // Build expanded text: file contents first, then original message
    let mut expanded = String::new();
    for (path, content) in &file_contents {
        expanded.push_str(&format!("<file path=\"{path}\">\n{content}\n</file>\n\n"));
    }
    expanded.push_str(text);
    expanded
}

fn event_reader_loop(tx: mpsc::UnboundedSender<Msg>) {
    loop {
        match event::read() {
            Ok(Event::Key(key)) => {
                if tx.send(Msg::Key(key)).is_err() {
                    break;
                }
            }
            Ok(Event::Resize(w, h)) => {
                if tx.send(Msg::Resize(w, h)).is_err() {
                    break;
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_file_mentions_no_mentions() {
        assert_eq!(expand_file_mentions("hello world"), "hello world");
    }

    #[test]
    fn test_expand_file_mentions_nonexistent_file() {
        // Non-existent file should be left as-is
        assert_eq!(
            expand_file_mentions("check @/nonexistent/path/xyz.rs"),
            "check @/nonexistent/path/xyz.rs"
        );
    }

    #[test]
    fn test_expand_file_mentions_email_not_expanded() {
        // Email addresses should NOT be treated as file mentions
        assert_eq!(
            expand_file_mentions("send to user@example.com"),
            "send to user@example.com"
        );
    }

    #[test]
    fn test_expand_file_mentions_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "file contents here").unwrap();
        let path_str = file_path.to_str().unwrap();

        let input = format!("read @{path_str} please");
        let expanded = expand_file_mentions(&input);

        assert!(expanded.contains("<file path="), "Expected file tag");
        assert!(expanded.contains("file contents here"), "Expected file contents");
        assert!(expanded.contains(&input), "Expected original text preserved");
    }

    #[test]
    fn test_expand_file_mentions_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("start.txt");
        std::fs::write(&file_path, "start content").unwrap();
        let path_str = file_path.to_str().unwrap();

        let input = format!("@{path_str}");
        let expanded = expand_file_mentions(&input);

        assert!(expanded.contains("start content"), "Expected file contents");
    }
}
