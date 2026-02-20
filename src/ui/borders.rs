use ratatui::style::Style;
use ratatui::symbols::border;
use ratatui::widgets::{Block, Borders};

use crate::theme::Theme;

pub fn themed_block<'a>(title: &'a str, focused: bool, theme: &Theme) -> Block<'a> {
    let border_color = if focused {
        theme.border_focused
    } else {
        theme.border
    };

    let title_style = Style::default().fg(if focused { theme.primary } else { theme.border });

    Block::default()
        .title(title)
        .title_style(title_style)
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(Style::default().fg(border_color).bg(theme.background))
        .style(Style::default().bg(theme.background))
}
