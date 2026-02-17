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
use crate::history::InputHistory;
use crate::theme::Theme;
use crate::todo::TodoTracker;
use crate::ui;
use crate::ui::header::{COMPACT_HEADER_HEIGHT, HEADER_HEIGHT};
use crate::ui::input::InputEditor;
use crate::ui::overlay::{OverlayItem, OverlayState};
use crate::ui::toast::Toast;

/// Built-in workflow templates: (name, description, prompt).
const WORKFLOW_TEMPLATES: &[(&str, &str, &str)] = &[
    (
        "Code Review",
        "Review recent changes for bugs and improvements",
        "Review the recent code changes in this session. Look for bugs, logic errors, security issues, and suggest improvements. Be specific about file and line numbers.",
    ),
    (
        "Write Tests",
        "Generate tests for recent changes",
        "Write comprehensive tests for the code I've been working on in this session. Cover edge cases, error paths, and the main happy path. Use the project's existing test patterns.",
    ),
    (
        "Debug Issue",
        "Systematic debugging of a problem",
        "Help me debug the current issue. Start by understanding the symptoms, then systematically trace through the code to identify the root cause. Suggest a fix with explanation.",
    ),
    (
        "Refactor",
        "Improve code structure and readability",
        "Analyze the code I've been working on and suggest refactoring improvements. Focus on reducing complexity, improving naming, extracting functions, and following project conventions. Show before/after.",
    ),
    (
        "Security Audit",
        "Check for security vulnerabilities",
        "Perform a security audit of the recent changes. Check for OWASP top 10 vulnerabilities, input validation issues, authentication/authorization gaps, and data exposure risks.",
    ),
    (
        "Performance Review",
        "Analyze code for performance issues",
        "Review the recent code for performance issues. Look for N+1 queries, unnecessary allocations, blocking operations, missing caching opportunities, and algorithmic complexity problems.",
    ),
    (
        "Documentation",
        "Generate documentation for recent code",
        "Generate clear documentation for the code I've been working on. Include function/module docs, usage examples, and any important caveats. Follow the project's existing documentation style.",
    ),
    (
        "Explain Codebase",
        "Get an overview of the project structure",
        "Explain the overall architecture of this codebase. Cover the main modules, how they interact, key design patterns used, and the data flow. Include file paths for each major component.",
    ),
    (
        "Git Summary",
        "Summarize recent git changes",
        "Look at the recent git history and summarize what's changed. Group changes by feature/topic, note any breaking changes, and highlight anything that might need attention.",
    ),
    (
        "Dependency Check",
        "Audit project dependencies",
        "Review the project's dependencies. Check for outdated packages, known vulnerabilities, unused dependencies, and suggest any that could be replaced or removed.",
    ),
];

/// All known vanilla Claude Code slash commands with descriptions.
/// Used as fallback when system.init doesn't include all commands.
const KNOWN_SLASH_COMMANDS: &[(&str, &str)] = &[
    ("bug", "Report a bug to the Anthropic team"),
    ("clear", "Clear conversation history"),
    ("compact", "Compress conversation context"),
    ("config", "Open settings"),
    ("context", "Show context usage"),
    ("copy", "Copy last response to clipboard"),
    ("cost", "Show token usage and costs"),
    ("doctor", "Check installation health"),
    ("exit", "Exit Claude Code"),
    ("export", "Export conversation to file"),
    ("help", "Show available commands"),
    ("init", "Initialize project CLAUDE.md"),
    ("keybindings", "Configure keybindings"),
    ("mcp", "Manage MCP servers"),
    ("memory", "Edit CLAUDE.md memory files"),
    ("model", "Switch AI model"),
    ("permissions", "Show or update tool permissions"),
    ("plan", "Enter plan mode"),
    ("plugins", "Browse and manage plugins"),
    ("rename", "Rename current session"),
    ("resume", "Resume a previous session"),
    ("rewind", "Rewind to earlier state"),
    ("stats", "Show usage statistics"),
    ("status", "Show version and account info"),
    ("tasks", "Show background tasks"),
    ("terminal-setup", "Install Shift+Enter keybinding"),
    ("theme", "Change color theme"),
    ("todos", "List current TODOs"),
    ("usage", "Show plan usage and rate limits"),
    ("vim", "Toggle vim mode"),
];

enum Msg {
    ClaudeEvent(StreamEvent),
    ClaudeExited,
    Key(event::KeyEvent),
    Paste(String),
    Resize(u16, u16),
    Tick,
}

/// Actions for commands handled locally (not sent to Claude).
enum LocalAction {
    Clear,
    Help,
    ShowConfig,
    ShowModel,
    ShowMemory,
    ShowPlugins,
    Exit,
    ChangeTheme,
}

/// A parsed question from AskUserQuestion tool input.
#[derive(Clone)]
struct UserQuestion {
    question: String,
    #[allow(dead_code)]
    header: String,
    options: Vec<UserQuestionOption>,
    multi_select: bool,
}

/// A single option in a UserQuestion.
#[derive(Clone)]
struct UserQuestionOption {
    label: String,
    description: String,
}

/// Metadata for a plugin in the browser.
#[derive(Clone)]
pub struct PluginInfo {
    pub name: String,
    pub marketplace: String,
    pub description: String,
    pub is_mcp: bool,
    pub installed: bool,
    pub enabled: bool,
}

impl PluginInfo {
    pub fn full_name(&self) -> String {
        format!("{}@{}", self.name, self.marketplace)
    }

    pub fn status_icon(&self) -> &str {
        if self.installed && self.enabled {
            "[+]"
        } else if self.installed {
            "[-]"
        } else {
            "[ ]"
        }
    }
}

/// Content shown in the right split pane.
#[derive(Clone)]
pub enum SplitContent {
    /// Default: list of files touched in the session.
    FileContext(Vec<String>),
    /// File content preview (filename, lines).
    FilePreview(String, Vec<String>),
    /// Unified diff view.
    DiffView(Vec<String>),
}

/// Tracks a sub-agent spawned via the Task tool.
pub struct AgentTask {
    /// tool_use_id that created this agent.
    pub id: String,
    /// Short description from the Task tool input.
    pub description: String,
    /// Sub-agent type (e.g. "Bash", "Explore", "Plan").
    pub agent_type: String,
    /// When this agent was spawned.
    pub started: std::time::Instant,
    /// Whether this agent has completed.
    pub completed: bool,
}

/// What to do when a TextInput overlay is confirmed.
enum TextInputAction {
    RenameSession,
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
    HistorySearch {
        query: String,
        matches: Vec<String>,
        selected: usize,
    },
    CheckpointTimeline(OverlayState),
    TextInput {
        prompt: String,
        value: String,
        cursor: usize,
        action: TextInputAction,
    },
    UserQuestion {
        questions: Vec<UserQuestion>,
        current_question: usize,
        cursor: usize,
        /// For multi-select: tracks which options are toggled on.
        selected: Vec<bool>,
    },
    PluginBrowser {
        plugins: Vec<PluginInfo>,
        cursor: usize,
        scroll: usize,
    },
    WorkflowPicker(OverlayState),
    AgentDashboard {
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
    /// Resume a specific session by ID from CLI args.
    resume_session_id: Option<String>,
    /// Current git repo info (branch, dirty count).
    git_info: GitInfo,
    /// Frame counter at last git refresh (refresh every ~5s).
    git_last_refresh: u64,
    /// Tracks Claude's todo list from TodoWrite tool calls.
    todo_tracker: TodoTracker,
    /// Model name detected from the most recent MessageStart event.
    detected_model: Option<String>,
    /// Persistent input history for Up/Down arrow and Ctrl+R search.
    history: InputHistory,
    /// Current position when browsing history with Up/Down arrow (None = not browsing).
    history_browse_index: Option<usize>,
    /// Whether all tool result blocks are expanded (toggled with Ctrl+E).
    tools_expanded: bool,
    /// Tracks AskUserQuestion tool_use blocks pending user interaction.
    /// Maps tool_use_id → accumulated input JSON string.
    pending_user_questions: std::collections::HashMap<String, String>,
    /// Whether split pane mode is active (Ctrl+S).
    split_pane: bool,
    /// Content displayed in the right split pane.
    split_content: SplitContent,
    /// Scroll offset for the right split pane.
    split_scroll: usize,
    /// Tracks sub-agents spawned via the Task tool. Keyed by tool_use_id.
    agent_tasks: Vec<AgentTask>,
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
        resume_session_id: Option<String>,
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
            resume_session_id,
            git_info: GitInfo::gather(),
            git_last_refresh: 0,
            todo_tracker: TodoTracker::new(),
            detected_model: None,
            history: InputHistory::new(),
            history_browse_index: None,
            tools_expanded: false,
            pending_user_questions: std::collections::HashMap::new(),
            split_pane: false,
            split_content: SplitContent::FileContext(Vec::new()),
            split_scroll: 0,
            agent_tasks: Vec::new(),
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
            resume_session_id: self.resume_session_id.clone(),
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
                if let StreamEvent::SystemHook { ref subtype, ref hook_id } = event {
                    let name = hook_id.as_deref().unwrap_or("hook");
                    match subtype.as_str() {
                        "hook_started" => {
                            self.toast = Some(Toast::new(format!("Running hook: {name}")));
                        }
                        "hook_completed" => {
                            self.toast = Some(Toast::new(format!("Hook completed: {name}")));
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

                // Update todo tracker and track AskUserQuestion when tool_use blocks complete
                if let StreamEvent::ContentBlockStop { index } = &event {
                    if let Some(msg) = self.conversation.messages.last() {
                        if let Some(crate::claude::conversation::ContentBlock::ToolUse {
                            name, input, id,
                        }) = msg.content.get(*index)
                        {
                            if name == "TodoWrite" {
                                self.todo_tracker.apply_todo_write(input);
                            }
                            if name == "AskUserQuestion" {
                                self.pending_user_questions
                                    .insert(id.clone(), input.clone());
                            }
                            // Track sub-agent spawning via Task tool
                            if name == "Task" {
                                if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
                                    let description = value
                                        .get("description")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("agent task")
                                        .to_string();
                                    let agent_type = value
                                        .get("subagent_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    self.agent_tasks.push(AgentTask {
                                        id: id.clone(),
                                        description,
                                        agent_type,
                                        started: std::time::Instant::now(),
                                        completed: false,
                                    });
                                }
                            }
                        }
                    }
                }

                // Mark agent tasks complete when their ToolResult arrives
                if let StreamEvent::ToolResult { ref tool_use_id, .. } = event {
                    for task in &mut self.agent_tasks {
                        if task.id == *tool_use_id {
                            task.completed = true;
                        }
                    }
                }

                // Intercept ToolResult for AskUserQuestion — show interactive overlay
                if let StreamEvent::ToolResult { ref tool_use_id, .. } = event {
                    if let Some(input_json) = self.pending_user_questions.remove(tool_use_id) {
                        if let Some(questions) = parse_ask_user_questions(&input_json) {
                            if !questions.is_empty() {
                                let num_options = questions[0].options.len();
                                self.mode = AppMode::UserQuestion {
                                    questions,
                                    current_question: 0,
                                    cursor: 0,
                                    selected: vec![false; num_options],
                                };
                            }
                        }
                    }
                }

                // Auto-update split pane content based on tool results
                if self.split_pane {
                    self.update_split_content_from_event(&event);
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
            Msg::Paste(text) => {
                if matches!(self.mode, AppMode::Normal) {
                    self.input.insert_str(&text);
                    self.history_browse_index = None;
                    self.update_completions();
                }
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
            | AppMode::SessionPicker(_)
            | AppMode::CheckpointTimeline(_)
            | AppMode::WorkflowPicker(_) => self.handle_key_overlay(key).await,
            AppMode::TextViewer { .. } => self.handle_key_text_viewer(key),
            AppMode::HistorySearch { .. } => self.handle_key_history_search(key),
            AppMode::TextInput { .. } => self.handle_key_text_input(key).await,
            AppMode::UserQuestion { .. } => self.handle_key_user_question(key).await,
            AppMode::PluginBrowser { .. } => self.handle_key_plugin_browser(key).await,
            AppMode::AgentDashboard { .. } => self.handle_key_agent_dashboard(key),
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
            self.open_history_search();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('i') {
            self.open_instructions_viewer();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('m') {
            self.open_memory_viewer();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('f') {
            self.open_file_context_panel();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('w') {
            self.open_workflow_picker();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('p') {
            self.open_plugin_browser();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('d') {
            self.open_diff_viewer();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('e') {
            self.tools_expanded = !self.tools_expanded;
            let msg = if self.tools_expanded { "Tool output expanded" } else { "Tool output collapsed" };
            self.toast = Some(Toast::new(msg.to_string()));
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('a') {
            self.open_agent_dashboard();
            return Ok(());
        }

        if ctrl && key.code == KeyCode::Char('s') {
            self.split_pane = !self.split_pane;
            let msg = if self.split_pane { "Split pane enabled" } else { "Split pane closed" };
            self.toast = Some(Toast::new(msg.to_string()));
            return Ok(());
        }

        // Scrolling — Shift+PageUp/Down scrolls split pane, plain PageUp/Down scrolls conversation
        if self.split_pane && shift {
            match key.code {
                KeyCode::PageUp => {
                    self.split_scroll = self.split_scroll.saturating_sub(10);
                    return Ok(());
                }
                KeyCode::PageDown => {
                    self.split_scroll += 10;
                    return Ok(());
                }
                _ => {}
            }
        }
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

        // History browsing with Up/Down when input is empty (and no completion popup)
        if self.completion.is_none() && self.input.is_empty() {
            match key.code {
                KeyCode::Up => {
                    let idx = self.history_browse_index.map(|i| i + 1).unwrap_or(0);
                    if let Some(entry) = self.history.get_reverse(idx) {
                        self.input.set_content(entry);
                        self.history_browse_index = Some(idx);
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        // Down arrow while browsing history
        if self.completion.is_none() && self.history_browse_index.is_some() {
            if key.code == KeyCode::Down {
                if let Some(idx) = self.history_browse_index {
                    if idx == 0 {
                        // Past newest — clear input
                        self.input.set_content("");
                        self.history_browse_index = None;
                    } else {
                        let new_idx = idx - 1;
                        if let Some(entry) = self.history.get_reverse(new_idx) {
                            self.input.set_content(entry);
                            self.history_browse_index = Some(new_idx);
                        }
                    }
                }
                return Ok(());
            }
        }

        // Input handling
        match key.code {
            KeyCode::Enter if !shift => {
                if !self.input.is_empty() && !self.conversation.is_streaming() {
                    let text = self.input.take_content();
                    self.history.push(text.clone());
                    self.history_browse_index = None;

                    if let Some(action) = self.handle_local_command(&text) {
                        // Command handled locally
                        match action {
                            LocalAction::Clear => {
                                self.conversation = Conversation::new();
                                self.scroll_offset = 0;
                                self.auto_scroll = true;
                            }
                            LocalAction::Help => {
                                self.show_help_viewer();
                            }
                            LocalAction::ShowConfig => {
                                self.show_config_viewer();
                            }
                            LocalAction::ShowModel => {
                                let model = self.detected_model.as_deref()
                                    .or(self.model_override.as_deref())
                                    .or(self.config.model.as_deref())
                                    .unwrap_or("(default)");
                                self.toast = Some(Toast::new(format!("Model: {model}")));
                            }
                            LocalAction::ShowMemory => {
                                self.open_memory_viewer();
                            }
                            LocalAction::ShowPlugins => {
                                self.open_plugin_browser();
                            }
                            LocalAction::Exit => {
                                self.should_quit = true;
                            }
                            LocalAction::ChangeTheme => {
                                self.open_theme_picker();
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
                self.history_browse_index = None;
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
            | AppMode::SessionPicker(ref mut state)
            | AppMode::CheckpointTimeline(ref mut state)
            | AppMode::WorkflowPicker(ref mut state) => f(state),
            AppMode::Normal | AppMode::TextViewer { .. } | AppMode::HistorySearch { .. } | AppMode::TextInput { .. } | AppMode::UserQuestion { .. } | AppMode::PluginBrowser { .. } | AppMode::AgentDashboard { .. } => {}
        }
    }

    /// Build a unified list of completion items from slash commands and custom commands.
    fn all_completion_items(&self) -> Vec<CompletionItem> {
        // Start with commands reported by system.init (authoritative, highest priority)
        let mut items: Vec<CompletionItem> = self
            .slash_commands
            .iter()
            .map(|cmd| {
                // Look up description from known commands
                let desc = KNOWN_SLASH_COMMANDS
                    .iter()
                    .find(|(name, _)| *name == cmd.as_str())
                    .map(|(_, d)| d.to_string())
                    .unwrap_or_default();
                CompletionItem {
                    name: cmd.clone(),
                    description: desc,
                    score: 0,
                }
            })
            .collect();

        // Add known vanilla commands not already in the list (as fallback)
        for &(name, description) in KNOWN_SLASH_COMMANDS {
            if !items.iter().any(|i| i.name == name) {
                items.push(CompletionItem {
                    name: name.to_string(),
                    description: description.to_string(),
                    score: 0,
                });
            }
        }

        // Add custom commands from .md files (project/user level)
        for cmd in &self.custom_commands {
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
            "/help" => Some(LocalAction::Help),
            "/config" => Some(LocalAction::ShowConfig),
            "/model" => Some(LocalAction::ShowModel),
            "/memory" => Some(LocalAction::ShowMemory),
            "/plugins" => Some(LocalAction::ShowPlugins),
            "/exit" | "/quit" => Some(LocalAction::Exit),
            "/theme" => Some(LocalAction::ChangeTheme),
            _ => None,
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = usize::MAX;
    }

    fn clamp_scroll(&mut self) {
        let total = ui::claude_pane::total_lines_with_options(&self.conversation, 80, &self.theme, self.tools_expanded);
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

    /// Check whether a given slash command is available from Claude CLI.
    fn has_slash_command(&self, name: &str) -> bool {
        self.slash_commands.iter().any(|c| c == name)
    }

    fn open_action_menu(&mut self) {
        let mut items = vec![
            OverlayItem {
                label: "Continue Last Session".to_string(),
                value: "continue".to_string(),
                hint: String::new(),
            },
            OverlayItem {
                label: "Resume Session".to_string(),
                value: "resume".to_string(),
                hint: String::new(),
            },
        ];

        // Only show commands that are actually available in stream-json mode
        if self.has_slash_command("rename") {
            items.push(OverlayItem {
                label: "Rename Session".to_string(),
                value: "rename".to_string(),
                hint: String::new(),
            });
        }
        if self.has_slash_command("compact") {
            items.push(OverlayItem {
                label: "Compact Context".to_string(),
                value: "compact".to_string(),
                hint: String::new(),
            });
        }
        if self.has_slash_command("rewind") {
            items.push(OverlayItem {
                label: "Rewind to Checkpoint".to_string(),
                value: "rewind".to_string(),
                hint: String::new(),
            });
        }

        items.push(OverlayItem {
            label: "Workflow Templates".to_string(),
            value: "workflows".to_string(),
            hint: "Ctrl+W".to_string(),
        });
        items.push(OverlayItem {
            label: if self.split_pane { "Close Split Pane".to_string() } else { "Split Pane".to_string() },
            value: "split".to_string(),
            hint: "Ctrl+S".to_string(),
        });
        {
            let active = self.agent_tasks.iter().filter(|t| !t.completed).count();
            let total = self.agent_tasks.len();
            let label = if total == 0 {
                "Agent Dashboard".to_string()
            } else {
                format!("Agent Dashboard ({active} active / {total} total)")
            };
            items.push(OverlayItem {
                label,
                value: "agents".to_string(),
                hint: "Ctrl+A".to_string(),
            });
        }
        items.push(OverlayItem {
            label: "Switch Theme".to_string(),
            value: "theme".to_string(),
            hint: "Ctrl+T".to_string(),
        });
        items.push(OverlayItem {
            label: "Quit".to_string(),
            value: "quit".to_string(),
            hint: "Ctrl+Q".to_string(),
        });

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

    fn open_history_search(&mut self) {
        if self.history.len() == 0 {
            self.toast = Some(Toast::new("No history yet".to_string()));
            return;
        }
        let matches: Vec<String> = self.history.search("")
            .into_iter()
            .map(|(_, e)| e.to_string())
            .collect();
        self.mode = AppMode::HistorySearch {
            query: String::new(),
            matches,
            selected: 0,
        };
    }

    fn refresh_history_matches(&mut self) {
        if let AppMode::HistorySearch { ref query, ref mut matches, ref mut selected } = self.mode {
            *matches = self.history.search(query)
                .into_iter()
                .map(|(_, e)| e.to_string())
                .collect();
            *selected = (*selected).min(matches.len().saturating_sub(1));
        }
    }

    fn handle_key_history_search(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Enter => {
                let selected_text = if let AppMode::HistorySearch { ref matches, selected, .. } = self.mode {
                    matches.get(selected).cloned()
                } else {
                    None
                };
                self.mode = AppMode::Normal;
                if let Some(text) = selected_text {
                    self.input.set_content(&text);
                }
            }
            KeyCode::Up => {
                if let AppMode::HistorySearch { ref matches, ref mut selected, .. } = self.mode {
                    if !matches.is_empty() {
                        *selected = selected.checked_sub(1).unwrap_or(matches.len() - 1);
                    }
                }
            }
            KeyCode::Down => {
                if let AppMode::HistorySearch { ref matches, ref mut selected, .. } = self.mode {
                    if !matches.is_empty() {
                        *selected = (*selected + 1) % matches.len();
                    }
                }
            }
            KeyCode::Backspace => {
                if let AppMode::HistorySearch { ref mut query, .. } = self.mode {
                    query.pop();
                }
                self.refresh_history_matches();
            }
            KeyCode::Char(c) => {
                if let AppMode::HistorySearch { ref mut query, .. } = self.mode {
                    query.push(c);
                }
                self.refresh_history_matches();
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_key_text_input(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Enter => {
                let mode = std::mem::replace(&mut self.mode, AppMode::Normal);
                if let AppMode::TextInput { value, action, .. } = mode {
                    if !value.trim().is_empty() {
                        self.execute_text_input_action(action, &value).await?;
                    }
                }
            }
            KeyCode::Backspace => {
                if let AppMode::TextInput { ref mut value, ref mut cursor, .. } = self.mode {
                    if *cursor > 0 {
                        value.remove(*cursor - 1);
                        *cursor -= 1;
                    }
                }
            }
            KeyCode::Left => {
                if let AppMode::TextInput { ref mut cursor, .. } = self.mode {
                    *cursor = cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if let AppMode::TextInput { ref value, ref mut cursor, .. } = self.mode {
                    if *cursor < value.len() {
                        *cursor += 1;
                    }
                }
            }
            KeyCode::Home => {
                if let AppMode::TextInput { ref mut cursor, .. } = self.mode {
                    *cursor = 0;
                }
            }
            KeyCode::End => {
                if let AppMode::TextInput { ref value, ref mut cursor, .. } = self.mode {
                    *cursor = value.len();
                }
            }
            KeyCode::Char(c) => {
                if let AppMode::TextInput { ref mut value, ref mut cursor, .. } = self.mode {
                    value.insert(*cursor, c);
                    *cursor += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_key_user_question(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                // Dismiss without answering — Claude already got an error result
                self.mode = AppMode::Normal;
            }
            KeyCode::Up => {
                if let AppMode::UserQuestion { ref mut cursor, ref questions, current_question, .. } = self.mode {
                    if let Some(q) = questions.get(current_question) {
                        if !q.options.is_empty() {
                            *cursor = cursor.checked_sub(1).unwrap_or(q.options.len() - 1);
                        }
                    }
                }
            }
            KeyCode::Down => {
                if let AppMode::UserQuestion { ref mut cursor, ref questions, current_question, .. } = self.mode {
                    if let Some(q) = questions.get(current_question) {
                        if !q.options.is_empty() {
                            *cursor = (*cursor + 1) % q.options.len();
                        }
                    }
                }
            }
            KeyCode::Char(' ') => {
                // Toggle selection for multi-select
                if let AppMode::UserQuestion { ref mut selected, cursor, ref questions, current_question, .. } = self.mode {
                    if let Some(q) = questions.get(current_question) {
                        if q.multi_select {
                            if let Some(s) = selected.get_mut(cursor) {
                                *s = !*s;
                            }
                        }
                    }
                }
            }
            KeyCode::Enter => {
                let mode = std::mem::replace(&mut self.mode, AppMode::Normal);
                if let AppMode::UserQuestion { questions, current_question, cursor, selected } = mode {
                    if let Some(q) = questions.get(current_question) {
                        let answer = if q.multi_select {
                            // Collect all toggled options
                            let answers: Vec<&str> = q.options.iter()
                                .enumerate()
                                .filter(|(i, _)| selected.get(*i).copied().unwrap_or(false))
                                .map(|(_, opt)| opt.label.as_str())
                                .collect();
                            if answers.is_empty() {
                                // If nothing toggled, use cursor position
                                q.options.get(cursor).map(|o| o.label.as_str()).unwrap_or("").to_string()
                            } else {
                                answers.join(", ")
                            }
                        } else {
                            q.options.get(cursor).map(|o| o.label.clone()).unwrap_or_default()
                        };

                        if !answer.is_empty() {
                            // Send the user's answer as a regular message
                            let response = format!("{}: {}", q.question, answer);
                            self.conversation.push_user_message(response.clone());
                            if let Some(ref mut claude) = self.claude {
                                claude.send_message(&response).await?;
                            }
                            self.scroll_to_bottom();
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn execute_text_input_action(&mut self, action: TextInputAction, value: &str) -> Result<()> {
        match action {
            TextInputAction::RenameSession => {
                if !self.has_slash_command("rename") {
                    self.toast = Some(Toast::new("/rename not available".to_string()));
                    return Ok(());
                }
                let cmd = format!("/rename {}", value);
                self.pending_slash_command = Some(cmd.clone());
                if let Some(ref mut claude) = self.claude {
                    claude.send_message(&cmd).await?;
                }
                self.toast = Some(Toast::new(format!("Renamed session to \"{}\"", value)));
            }
        }
        Ok(())
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
                        "rename" => {
                            self.mode = AppMode::TextInput {
                                prompt: "Session name".to_string(),
                                value: String::new(),
                                cursor: 0,
                                action: TextInputAction::RenameSession,
                            };
                        }
                        "compact" => {
                            self.pending_slash_command = Some("/compact".to_string());
                            if let Some(ref mut claude) = self.claude {
                                claude.send_message("/compact").await?;
                            }
                            self.toast = Some(Toast::new("Compacting context...".to_string()));
                        }
                        "rewind" => self.open_checkpoint_timeline(),
                        "workflows" => self.open_workflow_picker(),
                        "split" => {
                            self.split_pane = !self.split_pane;
                            let msg = if self.split_pane { "Split pane enabled" } else { "Split pane closed" };
                            self.toast = Some(Toast::new(msg.to_string()));
                        }
                        "agents" => self.open_agent_dashboard(),
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
            AppMode::CheckpointTimeline(state) => {
                if let Some(value) = state.selected_value() {
                    // value is the turn number (1-based)
                    let cmd = format!("/rewind {}", value);
                    self.pending_slash_command = Some(cmd.clone());
                    if let Some(ref mut claude) = self.claude {
                        claude.send_message(&cmd).await?;
                    }
                    self.toast = Some(Toast::new(format!("Rewinding to turn {}...", value)));
                }
            }
            AppMode::WorkflowPicker(state) => {
                if let Some(value) = state.selected_value() {
                    // value is the workflow prompt text
                    self.conversation.push_user_message(value.clone());
                    self.auto_scroll = true;
                    self.scroll_to_bottom();
                    if let Some(ref mut claude) = self.claude {
                        claude.send_message(&value).await?;
                    }
                }
            }
            AppMode::Normal | AppMode::TextViewer { .. } | AppMode::HistorySearch { .. } | AppMode::TextInput { .. } | AppMode::UserQuestion { .. } | AppMode::PluginBrowser { .. } | AppMode::AgentDashboard { .. } => {}
        }
        Ok(())
    }

    fn show_help_viewer(&mut self) {
        let mut lines = vec![
            "# Available Commands".to_string(),
            String::new(),
            "## Slash Commands".to_string(),
        ];
        // Known commands
        for &(name, description) in KNOWN_SLASH_COMMANDS {
            let available = self.slash_commands.iter().any(|c| c == name);
            let marker = if available { " " } else { "?" };
            lines.push(format!(" {marker} /{name:20} {description}"));
        }
        // Custom commands
        if !self.custom_commands.is_empty() {
            lines.push(String::new());
            lines.push("## Custom Commands".to_string());
            for cmd in &self.custom_commands {
                let desc = if cmd.description.is_empty() {
                    "(no description)".to_string()
                } else {
                    cmd.description.clone()
                };
                lines.push(format!("   /{:20} {desc}", cmd.name));
            }
        }
        lines.push(String::new());
        lines.push("## Keyboard Shortcuts".to_string());
        lines.push("   Ctrl+Q              Quit".to_string());
        lines.push("   Ctrl+K              Action menu".to_string());
        lines.push("   Ctrl+T              Theme picker".to_string());
        lines.push("   Ctrl+R              History search".to_string());
        lines.push("   Ctrl+I              CLAUDE.md viewer".to_string());
        lines.push("   Ctrl+M              Auto-memory viewer".to_string());
        lines.push("   Ctrl+P              Plugin browser".to_string());
        lines.push("   Ctrl+W              Workflow templates".to_string());
        lines.push("   Ctrl+S              Toggle split pane".to_string());
        lines.push("   Ctrl+A              Agent dashboard".to_string());
        lines.push("   Ctrl+F              File context panel".to_string());
        lines.push("   Ctrl+D              Diff viewer".to_string());
        lines.push("   Ctrl+E              Toggle tool blocks".to_string());
        lines.push("   PageUp/PageDown     Scroll conversation".to_string());
        lines.push("   Shift+Enter         Insert newline".to_string());
        lines.push(String::new());
        lines.push("? = may not be available in stream-json mode".to_string());

        self.mode = AppMode::TextViewer {
            title: "Help".to_string(),
            lines,
            scroll: 0,
        };
    }

    fn show_config_viewer(&mut self) {
        let config_path = crate::config::Config::default_path();
        let content = std::fs::read_to_string(&config_path).unwrap_or_else(|_| {
            format!(
                "# Config file not found\n# Create it at: {}\n#\n# Example:\n# command = \"claude\"\n# theme = \"catppuccin-mocha\"\n# fps = 30\n# model = \"claude-sonnet-4-5-20250929\"\n# permission_mode = \"default\"",
                config_path.display()
            )
        });
        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        self.mode = AppMode::TextViewer {
            title: format!("Config ({})", config_path.display()),
            lines,
            scroll: 0,
        };
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

    fn open_memory_viewer(&mut self) {
        // Derive project memory directory from cwd
        let cwd = std::env::current_dir().unwrap_or_default();
        let project_key = cwd.to_string_lossy().replace('/', "-");
        let memory_dir = dirs::home_dir()
            .map(|h| h.join(".claude/projects").join(&project_key).join("memory"));

        let mut combined = String::new();
        let mut file_count = 0;

        if let Some(ref dir) = memory_dir {
            if dir.is_dir() {
                // Read all .md files, MEMORY.md first
                let mut files: Vec<_> = std::fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
                    .collect();
                files.sort_by(|a, b| {
                    let a_is_memory = a.file_name() == "MEMORY.md";
                    let b_is_memory = b.file_name() == "MEMORY.md";
                    b_is_memory.cmp(&a_is_memory).then(a.file_name().cmp(&b.file_name()))
                });

                for entry in files {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if file_count > 0 {
                            combined.push('\n');
                        }
                        combined.push_str(&format!("═══ {} ═══\n\n", name));
                        combined.push_str(&content);
                        file_count += 1;
                    }
                }
            }
        }

        if file_count == 0 {
            self.toast = Some(Toast::new("No memory files found".to_string()));
            return;
        }

        let lines: Vec<String> = combined.lines().map(|l| l.to_string()).collect();
        self.mode = AppMode::TextViewer {
            title: format!("Auto-Memory ({file_count} files)"),
            lines,
            scroll: 0,
        };
    }

    fn discover_plugins() -> Vec<PluginInfo> {
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return Vec::new(),
        };
        let marketplaces_dir = home.join(".claude/plugins/marketplaces");
        let installed_path = home.join(".claude/plugins/installed_plugins.json");
        let settings_path = home.join(".claude/settings.json");

        // Parse installed plugins
        let installed: std::collections::HashSet<String> = std::fs::read_to_string(&installed_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("plugins")?.as_object().cloned())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();

        // Parse enabled plugins
        let enabled: std::collections::HashSet<String> = std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("enabledPlugins")?.as_object().cloned())
            .map(|m| {
                m.into_iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .map(|(k, _)| k)
                    .collect()
            })
            .unwrap_or_default();

        let mut plugins = Vec::new();

        // Scan each marketplace
        if let Ok(entries) = std::fs::read_dir(&marketplaces_dir) {
            for entry in entries.flatten() {
                let marketplace_name = entry.file_name().to_string_lossy().to_string();
                let marketplace_path = entry.path();

                // Scan plugins/ and external_plugins/ subdirs
                for (subdir, is_mcp) in [("plugins", false), ("external_plugins", true)] {
                    let dir = marketplace_path.join(subdir);
                    if let Ok(plugin_entries) = std::fs::read_dir(&dir) {
                        for pe in plugin_entries.flatten() {
                            let name = pe.file_name().to_string_lossy().to_string();
                            let manifest_path = pe.path().join(".claude-plugin/plugin.json");
                            let description = std::fs::read_to_string(&manifest_path)
                                .ok()
                                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                                .and_then(|v| v.get("description")?.as_str().map(|s| s.to_string()))
                                .unwrap_or_default();

                            // Check if has .mcp.json (external plugins always MCP, but regular
                            // plugins might also have one)
                            let has_mcp = is_mcp || pe.path().join(".mcp.json").exists();

                            let full_name = format!("{}@{}", name, marketplace_name);
                            plugins.push(PluginInfo {
                                name: name.clone(),
                                marketplace: marketplace_name.clone(),
                                description,
                                is_mcp: has_mcp,
                                installed: installed.contains(&full_name),
                                enabled: enabled.contains(&full_name),
                            });
                        }
                    }
                }
            }
        }

        // Sort: enabled first, then installed, then available; alphabetically within groups
        plugins.sort_by(|a, b| {
            let rank = |p: &PluginInfo| -> u8 {
                if p.enabled { 0 } else if p.installed { 1 } else { 2 }
            };
            rank(a).cmp(&rank(b)).then(a.name.cmp(&b.name))
        });

        plugins
    }

    fn open_workflow_picker(&mut self) {
        let items: Vec<OverlayItem> = WORKFLOW_TEMPLATES
            .iter()
            .map(|(name, desc, prompt)| OverlayItem {
                label: name.to_string(),
                value: prompt.to_string(),
                hint: desc.to_string(),
            })
            .collect();
        self.mode = AppMode::WorkflowPicker(OverlayState::new(items, None));
    }

    fn open_agent_dashboard(&mut self) {
        if self.agent_tasks.is_empty() {
            self.toast = Some(Toast::new("No agent tasks in this session".to_string()));
            return;
        }
        self.mode = AppMode::AgentDashboard { scroll: 0 };
    }

    fn handle_key_agent_dashboard(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let AppMode::AgentDashboard { ref mut scroll } = self.mode {
                    *scroll = scroll.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let AppMode::AgentDashboard { ref mut scroll } = self.mode {
                    *scroll = (*scroll + 1).min(self.agent_tasks.len().saturating_sub(1));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn open_plugin_browser(&mut self) {
        let plugins = Self::discover_plugins();
        if plugins.is_empty() {
            self.toast = Some(Toast::new("No plugins found".to_string()));
            return;
        }
        self.mode = AppMode::PluginBrowser {
            plugins,
            cursor: 0,
            scroll: 0,
        };
    }

    async fn handle_key_plugin_browser(&mut self, key: event::KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = AppMode::Normal;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let AppMode::PluginBrowser { ref mut cursor, .. } = self.mode {
                    *cursor = cursor.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let AppMode::PluginBrowser { ref mut cursor, ref plugins, .. } = self.mode {
                    if *cursor + 1 < plugins.len() {
                        *cursor += 1;
                    }
                }
            }
            KeyCode::Enter => {
                // Open plugin README in TextViewer
                if let AppMode::PluginBrowser { ref plugins, cursor, .. } = self.mode {
                    if let Some(plugin) = plugins.get(cursor) {
                        let home = dirs::home_dir().unwrap_or_default();
                        let marketplace_dir = home.join(".claude/plugins/marketplaces").join(&plugin.marketplace);
                        let subdir = if plugin.is_mcp { "external_plugins" } else { "plugins" };
                        let readme_path = marketplace_dir.join(subdir).join(&plugin.name).join("README.md");

                        let content = std::fs::read_to_string(&readme_path)
                            .unwrap_or_else(|_| format!("# {}\n\n{}\n\nNo README available.", plugin.name, plugin.description));
                        let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                        self.mode = AppMode::TextViewer {
                            title: format!("{} ({})", plugin.name, plugin.marketplace),
                            lines,
                            scroll: 0,
                        };
                    }
                }
            }
            KeyCode::Char(' ') => {
                // Toggle enable/disable
                let cmd = if let AppMode::PluginBrowser { ref plugins, cursor, .. } = self.mode {
                    plugins.get(cursor).filter(|p| p.installed).map(|p| {
                        let action = if p.enabled { "disable" } else { "enable" };
                        (action.to_string(), p.full_name())
                    })
                } else {
                    None
                };
                if let Some((action, name)) = cmd {
                    let output = std::process::Command::new("claude")
                        .args(["plugin", &action, &name])
                        .env_remove("CLAUDECODE")
                        .env_remove("CLAUDE_CODE_ENTRYPOINT")
                        .output();
                    match output {
                        Ok(o) if o.status.success() => {
                            self.toast = Some(Toast::new(format!("Plugin {action}d: {name}")));
                            // Refresh the plugin list
                            let plugins = Self::discover_plugins();
                            self.mode = AppMode::PluginBrowser { plugins, cursor: 0, scroll: 0 };
                        }
                        Ok(o) => {
                            let err = String::from_utf8_lossy(&o.stderr);
                            self.toast = Some(Toast::new(format!("Failed: {err}")));
                        }
                        Err(e) => {
                            self.toast = Some(Toast::new(format!("Error: {e}")));
                        }
                    }
                }
            }
            KeyCode::Char('i') => {
                // Install uninstalled plugin
                let cmd = if let AppMode::PluginBrowser { ref plugins, cursor, .. } = self.mode {
                    plugins.get(cursor).filter(|p| !p.installed).map(|p| p.full_name())
                } else {
                    None
                };
                if let Some(name) = cmd {
                    self.toast = Some(Toast::new(format!("Installing {name}...")));
                    let output = std::process::Command::new("claude")
                        .args(["plugin", "install", &name])
                        .env_remove("CLAUDECODE")
                        .env_remove("CLAUDE_CODE_ENTRYPOINT")
                        .output();
                    match output {
                        Ok(o) if o.status.success() => {
                            self.toast = Some(Toast::new(format!("Installed: {name}")));
                            let plugins = Self::discover_plugins();
                            self.mode = AppMode::PluginBrowser { plugins, cursor: 0, scroll: 0 };
                        }
                        Ok(o) => {
                            let err = String::from_utf8_lossy(&o.stderr);
                            self.toast = Some(Toast::new(format!("Install failed: {err}")));
                        }
                        Err(e) => {
                            self.toast = Some(Toast::new(format!("Error: {e}")));
                        }
                    }
                }
            }
            KeyCode::Char('u') => {
                // Uninstall installed plugin
                let cmd = if let AppMode::PluginBrowser { ref plugins, cursor, .. } = self.mode {
                    plugins.get(cursor).filter(|p| p.installed).map(|p| p.full_name())
                } else {
                    None
                };
                if let Some(name) = cmd {
                    let output = std::process::Command::new("claude")
                        .args(["plugin", "uninstall", &name])
                        .env_remove("CLAUDECODE")
                        .env_remove("CLAUDE_CODE_ENTRYPOINT")
                        .output();
                    match output {
                        Ok(o) if o.status.success() => {
                            self.toast = Some(Toast::new(format!("Uninstalled: {name}")));
                            let plugins = Self::discover_plugins();
                            self.mode = AppMode::PluginBrowser { plugins, cursor: 0, scroll: 0 };
                        }
                        Ok(o) => {
                            let err = String::from_utf8_lossy(&o.stderr);
                            self.toast = Some(Toast::new(format!("Uninstall failed: {err}")));
                        }
                        Err(e) => {
                            self.toast = Some(Toast::new(format!("Error: {e}")));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
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

    /// Update split pane content based on incoming stream events.
    /// Reacts to tool executions: Edit → DiffView, Read/Write → FilePreview.
    fn update_split_content_from_event(&mut self, event: &StreamEvent) {
        use crate::claude::conversation::ContentBlock;

        // When a tool is about to execute (MessageStop with ToolUse), update the split pane
        if let StreamEvent::MessageStop = event {
            if let Some(msg) = self.conversation.messages.last() {
                if let Some(ContentBlock::ToolUse { name, input, .. }) = msg.content.last() {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
                        match name.as_str() {
                            "Edit" => {
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
                                let mut lines = vec![format!("--- {file_path}"), format!("+++ {file_path}")];
                                let ops = crate::diff::diff_lines(old, new);
                                for line in crate::diff::format_unified(&ops).lines() {
                                    lines.push(line.to_string());
                                }
                                self.split_content = SplitContent::DiffView(lines);
                                self.split_scroll = 0;
                            }
                            "Read" => {
                                let file_path = value
                                    .get("file_path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                // Content will appear in tool result; show placeholder
                                self.split_content = SplitContent::FilePreview(
                                    file_path,
                                    vec!["Reading file...".to_string()],
                                );
                                self.split_scroll = 0;
                            }
                            "Write" => {
                                let file_path = value
                                    .get("file_path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();
                                let content = value
                                    .get("content")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                                self.split_content = SplitContent::FilePreview(file_path, lines);
                                self.split_scroll = 0;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // When a ToolResult arrives for a Read, populate with the actual content
        if let StreamEvent::ToolResult { ref tool_use_id, ref content, .. } = event {
            // Find the matching ToolUse to check if it was a Read
            for msg in self.conversation.messages.iter().rev() {
                for block in msg.content.iter().rev() {
                    if let ContentBlock::ToolUse { id, name, .. } = block {
                        if id == tool_use_id && name == "Read" {
                            if let SplitContent::FilePreview(ref path, _) = self.split_content {
                                let path = path.clone();
                                let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
                                self.split_content = SplitContent::FilePreview(path, lines);
                                self.split_scroll = 0;
                            }
                            return;
                        }
                    }
                }
            }
        }
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

    fn open_checkpoint_timeline(&mut self) {
        use crate::claude::conversation::{ContentBlock, Role};

        // Build checkpoint list from user messages
        let mut turn_number = 0u32;
        let mut items: Vec<OverlayItem> = Vec::new();

        for msg in &self.conversation.messages {
            if msg.role != Role::User {
                continue;
            }
            turn_number += 1;

            // Extract first line of user text as preview
            let preview = msg.content.iter().find_map(|block| {
                if let ContentBlock::Text(text) = block {
                    let first_line = text.trim().lines().next().unwrap_or("").to_string();
                    if first_line.len() > 60 {
                        Some(format!("{}...", &first_line[..57]))
                    } else {
                        Some(first_line)
                    }
                } else {
                    None
                }
            }).unwrap_or_else(|| "(empty)".to_string());

            items.push(OverlayItem {
                label: format!("Turn {} — {}", turn_number, preview),
                value: turn_number.to_string(),
                hint: String::new(),
            });
        }

        if items.is_empty() {
            self.toast = Some(Toast::new("No checkpoints available".to_string()));
            return;
        }

        // Oldest first (chronological order)
        self.mode = AppMode::CheckpointTimeline(OverlayState::new(items, None));
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
            AppMode::CheckpointTimeline(state) => Some(("Rewind to Checkpoint", state)),
            AppMode::WorkflowPicker(state) => Some(("Workflow Templates", state)),
            AppMode::Normal | AppMode::TextViewer { .. } | AppMode::HistorySearch { .. } | AppMode::TextInput { .. } | AppMode::UserQuestion { .. } | AppMode::PluginBrowser { .. } | AppMode::AgentDashboard { .. } => None,
        };

        // Clamp scroll before rendering
        let term_size = terminal.size()?;
        let header_h = if self.conversation.messages.is_empty() { HEADER_HEIGHT } else { COMPACT_HEADER_HEIGHT };
        let visible_height = term_size.height.saturating_sub(header_h + 4) as usize;
        let total_conv_lines = ui::claude_pane::total_lines_with_options(
            &self.conversation,
            term_size.width.saturating_sub(4) as usize,
            &self.theme,
            self.tools_expanded,
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
        let tools_expanded = self.tools_expanded;
        let text_viewer = match &self.mode {
            AppMode::TextViewer {
                title,
                lines,
                scroll,
            } => Some((title.as_str(), lines.as_slice(), *scroll)),
            _ => None,
        };
        let history_search = match &self.mode {
            AppMode::HistorySearch { query, matches, selected } => {
                Some((query.as_str(), matches.as_slice(), *selected))
            }
            _ => None,
        };
        let text_input = match &self.mode {
            AppMode::TextInput { prompt, value, cursor, .. } => {
                Some((prompt.as_str(), value.as_str(), *cursor))
            }
            _ => None,
        };
        let user_question = match &self.mode {
            AppMode::UserQuestion { questions, current_question, cursor, selected } => {
                questions.get(*current_question).map(|q| (q, *cursor, selected.as_slice()))
            }
            _ => None,
        };
        let plugin_browser = match &self.mode {
            AppMode::PluginBrowser { plugins, cursor, scroll } => {
                Some((plugins.as_slice(), *cursor, *scroll))
            }
            _ => None,
        };
        let agent_dashboard = match &self.mode {
            AppMode::AgentDashboard { scroll } => Some((&self.agent_tasks, *scroll)),
            _ => None,
        };
        let split_content = if self.split_pane { Some(&self.split_content) } else { None };
        let split_scroll = self.split_scroll;

        terminal.draw(|frame| {
            let active_tool = conversation.active_tool_name()
                .map(|name| (name, conversation.tool_elapsed_secs().unwrap_or(0)));
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
                tools_expanded,
                active_tool,
                split_content,
                split_scroll,
            );
            if let Some((title, state)) = overlay {
                ui::render_overlay(frame, title, state, theme);
            }
            if let Some((title, lines, scroll)) = text_viewer {
                ui::render_text_viewer(frame, title, lines, scroll, theme);
            }
            if let Some((query, matches, selected)) = history_search {
                ui::render_history_search(frame, query, matches, selected, theme);
            }
            if let Some((prompt, value, cursor)) = text_input {
                ui::render_text_input(frame, prompt, value, cursor, theme);
            }
            if let Some((question, cursor, selected)) = &user_question {
                let options: Vec<(&str, &str)> = question.options.iter()
                    .map(|o| (o.label.as_str(), o.description.as_str()))
                    .collect();
                ui::render_user_question(
                    frame,
                    &question.question,
                    &options,
                    *cursor,
                    selected,
                    question.multi_select,
                    theme,
                );
            }
            if let Some((plugins, cursor, scroll)) = plugin_browser {
                ui::render_plugin_browser(frame, plugins, cursor, scroll, theme);
            }
            if let Some((tasks, scroll)) = agent_dashboard {
                ui::render_agent_dashboard(frame, tasks, scroll, theme);
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

/// Parse AskUserQuestion tool input JSON into structured questions.
fn parse_ask_user_questions(input_json: &str) -> Option<Vec<UserQuestion>> {
    let val: serde_json::Value = serde_json::from_str(input_json).ok()?;
    let questions_arr = val.get("questions")?.as_array()?;
    let mut result = Vec::new();
    for q in questions_arr {
        let question = q.get("question")?.as_str()?.to_string();
        let header = q.get("header").and_then(|h| h.as_str()).unwrap_or("").to_string();
        let multi_select = q.get("multiSelect").and_then(|m| m.as_bool()).unwrap_or(false);
        let options_arr = q.get("options")?.as_array()?;
        let mut options = Vec::new();
        for opt in options_arr {
            let label = opt.get("label")?.as_str()?.to_string();
            let description = opt.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
            options.push(UserQuestionOption { label, description });
        }
        result.push(UserQuestion {
            question,
            header,
            options,
            multi_select,
        });
    }
    Some(result)
}

fn event_reader_loop(tx: mpsc::UnboundedSender<Msg>) {
    loop {
        match event::read() {
            Ok(Event::Key(key)) => {
                if tx.send(Msg::Key(key)).is_err() {
                    break;
                }
            }
            Ok(Event::Paste(text)) => {
                if tx.send(Msg::Paste(text)).is_err() {
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

    #[test]
    fn test_parse_ask_user_questions_single() {
        let json = r#"{"questions":[{"question":"Which approach?","header":"Approach","options":[{"label":"Option A","description":"First option"},{"label":"Option B","description":"Second option"}],"multiSelect":false}]}"#;
        let questions = parse_ask_user_questions(json).unwrap();
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "Which approach?");
        assert_eq!(questions[0].options.len(), 2);
        assert_eq!(questions[0].options[0].label, "Option A");
        assert_eq!(questions[0].options[1].description, "Second option");
        assert!(!questions[0].multi_select);
    }

    #[test]
    fn test_parse_ask_user_questions_multi_select() {
        let json = r#"{"questions":[{"question":"Which features?","header":"Features","options":[{"label":"A"},{"label":"B"},{"label":"C"}],"multiSelect":true}]}"#;
        let questions = parse_ask_user_questions(json).unwrap();
        assert_eq!(questions.len(), 1);
        assert!(questions[0].multi_select);
        assert_eq!(questions[0].options.len(), 3);
    }

    #[test]
    fn test_parse_ask_user_questions_invalid() {
        assert!(parse_ask_user_questions("not json").is_none());
        assert!(parse_ask_user_questions(r#"{"questions":[]}"#).unwrap().is_empty());
    }
}
