use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::claude::conversation::Conversation;
use crate::claude::events::StreamEvent;
use crate::claude::process::ClaudeProcess;
use crate::config::Config;
use crate::theme::Theme;
use crate::ui;
use crate::ui::header::HEADER_HEIGHT;
use crate::ui::input::InputEditor;
use crate::ui::overlay::{OverlayItem, OverlayState};

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
}

/// Tracks slash command completion state.
pub struct CompletionState {
    pub matches: Vec<String>,
    pub selected: usize,
}

impl CompletionState {
    fn new(matches: Vec<String>) -> Self {
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
        self.matches.get(self.selected).map(|s| s.as_str())
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
    completion: Option<CompletionState>,
}

impl App {
    pub fn new(config: Config, theme: Theme, theme_name: String, command: String) -> Self {
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
            completion: None,
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

        // Spawn Claude process
        let (claude_process, mut event_rx) = ClaudeProcess::spawn(&self.command)?;
        self.claude = Some(claude_process);

        // Forward Claude events to the main channel
        let tx_claude = tx.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if tx_claude.send(Msg::ClaudeEvent(event)).is_err() {
                    break;
                }
            }
            let _ = tx_claude.send(Msg::ClaudeExited);
        });

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

    async fn update(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::ClaudeEvent(event) => {
                // Extract slash commands from SystemInit before passing to conversation
                if let StreamEvent::SystemInit { ref slash_commands } = event {
                    self.slash_commands = slash_commands.clone();
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
            }
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: event::KeyEvent) -> Result<()> {
        match &self.mode {
            AppMode::Normal => self.handle_key_normal(key).await,
            AppMode::ActionMenu(_) | AppMode::ThemePicker(_) => self.handle_key_overlay(key),
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
                    } else if text.starts_with('/') {
                        // Slash command — send to Claude but don't add as user message
                        self.auto_scroll = true;
                        self.scroll_to_bottom();
                        if let Some(ref mut claude) = self.claude {
                            claude.send_message(&text).await?;
                        }
                    } else {
                        // Normal user message
                        self.conversation.push_user_message(text.clone());
                        self.auto_scroll = true;
                        self.scroll_to_bottom();
                        if let Some(ref mut claude) = self.claude {
                            claude.send_message(&text).await?;
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

    fn handle_key_overlay(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
            }
            KeyCode::Enter => {
                self.confirm_overlay()?;
            }
            KeyCode::Up => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.move_up();
                }
                self.preview_theme();
            }
            KeyCode::Down => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.move_down();
                }
                self.preview_theme();
            }
            KeyCode::Backspace => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) =
                    self.mode
                {
                    state.type_char(c);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Update slash command completions based on current input text.
    fn update_completions(&mut self) {
        let content = self.input.content();
        if !content.starts_with('/') || content.contains(' ') || content.contains('\n') {
            self.completion = None;
            return;
        }

        let prefix = &content[1..]; // strip the leading '/'
        let matches: Vec<String> = self
            .slash_commands
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .cloned()
            .collect();

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

    fn confirm_overlay(&mut self) -> Result<()> {
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
                        "theme" => self.open_theme_picker(),
                        "quit" => self.should_quit = true,
                        _ => {}
                    }
                }
            }
            AppMode::Normal => {}
        }
        Ok(())
    }

    fn view(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let theme = &self.theme;
        let frame_count = self.frame_count;
        let overlay = match &self.mode {
            AppMode::ActionMenu(state) => Some(("Actions", state)),
            AppMode::ThemePicker(state) => Some(("Select Theme", state)),
            AppMode::Normal => None,
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
            );
            if let Some((title, state)) = overlay {
                ui::render_overlay(frame, title, state, theme);
            }
        })?;

        Ok(())
    }
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
