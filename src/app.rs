use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::claude::conversation::Conversation;
use crate::config::Config;
use crate::pty::PtyProcess;
use crate::terminal::TerminalEmulator;
use crate::theme::Theme;
use crate::ui;
use crate::ui::header::HEADER_HEIGHT;
use crate::ui::overlay::{OverlayItem, OverlayState};

enum Msg {
    PtyOutput(Vec<u8>),
    PtyExited,
    Key(event::KeyEvent),
    Resize(u16, u16),
    Tick,
}

enum AppMode {
    Normal,
    ActionMenu(OverlayState),
    ThemePicker(OverlayState),
}

pub struct App {
    config: Config,
    theme: Theme,
    pty: Arc<Mutex<PtyProcess>>,
    emulator: TerminalEmulator,
    conversation: Conversation,
    scroll_offset: usize,
    should_quit: bool,
    frame_count: u64,
    mode: AppMode,
    theme_name: String,
}

impl App {
    pub fn new(config: Config, theme: Theme, theme_name: String, pty: PtyProcess, rows: u16, cols: u16) -> Self {
        // Reserve space for header, top/bottom border (2 rows), and status bar (1 row)
        let emu_rows = rows.saturating_sub(3 + HEADER_HEIGHT);
        let emu_cols = cols.saturating_sub(2); // account for left/right borders

        Self {
            config,
            theme,
            pty: Arc::new(Mutex::new(pty)),
            emulator: TerminalEmulator::new(emu_rows, emu_cols),
            conversation: Conversation::new(),
            scroll_offset: 0,
            should_quit: false,
            frame_count: 0,
            mode: AppMode::Normal,
            theme_name,
        }
    }

    pub async fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

        // Spawn PTY reader task
        let pty_reader = {
            let pty = self.pty.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            pty.take_reader()?
        };
        let tx_pty = tx.clone();
        std::thread::spawn(move || {
            pty_reader_loop(pty_reader, tx_pty);
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
            self.update(msg)?;
            if self.should_quit {
                break;
            }
            self.view(terminal)?;
        }

        Ok(())
    }

    fn update(&mut self, msg: Msg) -> Result<()> {
        match msg {
            Msg::PtyOutput(bytes) => {
                self.emulator.process(&bytes);
            }
            Msg::PtyExited => {
                self.should_quit = true;
            }
            Msg::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }
                self.handle_key(key)?;
            }
            Msg::Resize(width, height) => {
                let emu_rows = height.saturating_sub(3 + HEADER_HEIGHT);
                let emu_cols = width.saturating_sub(2);
                self.emulator.resize(emu_rows, emu_cols);
                if let Ok(pty) = self.pty.lock() {
                    let _ = pty.resize(emu_cols, emu_rows);
                }
            }
            Msg::Tick => {
                self.frame_count = self.frame_count.wrapping_add(1);
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: event::KeyEvent) -> Result<()> {
        match &self.mode {
            AppMode::Normal => self.handle_key_normal(key),
            AppMode::ActionMenu(_) | AppMode::ThemePicker(_) => self.handle_key_overlay(key),
        }
    }

    fn handle_key_normal(&mut self, key: event::KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

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

        // Pass through to PTY
        let bytes = key_to_bytes(&key);
        if !bytes.is_empty() {
            self.pty_write(&bytes)?;
        }
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
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                    state.move_up();
                }
                self.preview_theme();
            }
            KeyCode::Down => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                    state.move_down();
                }
                self.preview_theme();
            }
            KeyCode::Backspace => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                    state.backspace();
                }
            }
            KeyCode::Char(c) => {
                if let AppMode::ActionMenu(ref mut state) | AppMode::ThemePicker(ref mut state) = self.mode {
                    state.type_char(c);
                }
            }
            _ => {}
        }
        Ok(())
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

        let current_idx = items.iter().position(|i| i.value == self.theme_name).unwrap_or(0);
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

    fn pty_write(&self, data: &[u8]) -> Result<()> {
        let pty = self.pty.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        pty.write(data)
    }

    fn view(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let conversation = &self.conversation;
        let theme = &self.theme;
        let scroll_offset = self.scroll_offset;
        let frame_count = self.frame_count;
        let overlay = match &self.mode {
            AppMode::ActionMenu(state) => Some(("Actions", state)),
            AppMode::ThemePicker(state) => Some(("Select Theme", state)),
            AppMode::Normal => None,
        };

        terminal.draw(|frame| {
            ui::render(frame, conversation, theme, scroll_offset, frame_count);
            if let Some((title, state)) = overlay {
                ui::render_overlay(frame, title, state, theme);
            }
        })?;

        Ok(())
    }
}

/// Convert a crossterm KeyEvent into raw bytes to send to the PTY.
fn key_to_bytes(key: &event::KeyEvent) -> Vec<u8> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                // Ctrl+A = 0x01, Ctrl+B = 0x02, ..., Ctrl+Z = 0x1a
                let byte = (c as u8).wrapping_sub(b'a').wrapping_add(1);
                vec![byte]
            } else if alt {
                let mut bytes = vec![0x1b]; // ESC prefix for Alt
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                bytes
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                b"\x1b[Z".to_vec()
            } else {
                vec![b'\t']
            }
        }
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(n) => match n {
            1 => b"\x1bOP".to_vec(),
            2 => b"\x1bOQ".to_vec(),
            3 => b"\x1bOR".to_vec(),
            4 => b"\x1bOS".to_vec(),
            5 => b"\x1b[15~".to_vec(),
            6 => b"\x1b[17~".to_vec(),
            7 => b"\x1b[18~".to_vec(),
            8 => b"\x1b[19~".to_vec(),
            9 => b"\x1b[20~".to_vec(),
            10 => b"\x1b[21~".to_vec(),
            11 => b"\x1b[23~".to_vec(),
            12 => b"\x1b[24~".to_vec(),
            _ => vec![],
        },
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        _ => vec![],
    }
}

fn pty_reader_loop(mut reader: Box<dyn Read + Send>, tx: mpsc::UnboundedSender<Msg>) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let _ = tx.send(Msg::PtyExited);
                break;
            }
            Ok(n) => {
                let _ = tx.send(Msg::PtyOutput(buf[..n].to_vec()));
            }
            Err(_) => {
                let _ = tx.send(Msg::PtyExited);
                break;
            }
        }
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
