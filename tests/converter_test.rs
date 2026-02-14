use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

// We need to test the converter directly, so we replicate its logic here
// since it's in a binary crate. In a real project you'd extract to a lib crate.

fn convert_color(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(idx) => Color::Indexed(idx),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn render_screen(screen: &vt100::Screen, buf: &mut Buffer, area: Rect) {
    let rows = area.height.min(screen.size().0);
    let cols = area.width.min(screen.size().1);

    for row in 0..rows {
        for col in 0..cols {
            let cell = screen.cell(row, col);
            let Some(cell) = cell else { continue };

            let x = area.x + col;
            let y = area.y + row;
            if x >= area.right() || y >= area.bottom() {
                continue;
            }

            let contents = cell.contents();
            if contents.is_empty() && col > 0 {
                continue;
            }

            let fg = convert_color(cell.fgcolor());
            let bg = convert_color(cell.bgcolor());
            let mut modifiers = Modifier::empty();
            if cell.bold() {
                modifiers |= Modifier::BOLD;
            }
            if cell.italic() {
                modifiers |= Modifier::ITALIC;
            }
            if cell.underline() {
                modifiers |= Modifier::UNDERLINED;
            }

            let style = ratatui::style::Style::default()
                .fg(fg)
                .bg(bg)
                .add_modifier(modifiers);

            let buf_cell = &mut buf[(x, y)];
            if contents.is_empty() {
                buf_cell.set_symbol(" ");
            } else {
                buf_cell.set_symbol(&contents);
            }
            buf_cell.set_style(style);
        }
    }
}

#[test]
fn test_snapshot_plain_text() {
    let mut parser = vt100::Parser::new(3, 20, 0);
    parser.process(b"Hello World\r\nLine Two\r\nLine Three");

    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    render_screen(parser.screen(), &mut buf, area);

    assert_eq!(buf[(0, 0)].symbol(), "H");
    assert_eq!(buf[(5, 0)].symbol(), " ");
    assert_eq!(buf[(6, 0)].symbol(), "W");
    assert_eq!(buf[(0, 1)].symbol(), "L");
    assert_eq!(buf[(0, 2)].symbol(), "L");
}

#[test]
fn test_snapshot_ansi_colors() {
    let mut parser = vt100::Parser::new(3, 20, 0);
    // Green text: ESC[32m
    parser.process(b"\x1b[32mGreen\x1b[0m Normal");

    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    render_screen(parser.screen(), &mut buf, area);

    // 'G' should be green (color index 2)
    let g_cell = &buf[(0, 0)];
    assert_eq!(g_cell.symbol(), "G");
    assert_eq!(g_cell.style().fg.unwrap(), Color::Indexed(2));

    // 'N' in "Normal" should be default
    let n_cell = &buf[(6, 0)];
    assert_eq!(n_cell.symbol(), "N");
    assert_eq!(n_cell.style().fg.unwrap(), Color::Reset);
}

#[test]
fn test_snapshot_bold_underline() {
    let mut parser = vt100::Parser::new(3, 20, 0);
    // Bold + Underline: ESC[1;4m
    parser.process(b"\x1b[1;4mStyled\x1b[0m");

    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    render_screen(parser.screen(), &mut buf, area);

    let s_cell = &buf[(0, 0)];
    assert_eq!(s_cell.symbol(), "S");
    assert!(s_cell.style().add_modifier.contains(Modifier::BOLD));
    assert!(s_cell.style().add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn test_snapshot_256_color() {
    let mut parser = vt100::Parser::new(3, 20, 0);
    // 256-color: ESC[38;5;196m (bright red, index 196)
    parser.process(b"\x1b[38;5;196mRed\x1b[0m");

    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    render_screen(parser.screen(), &mut buf, area);

    let r_cell = &buf[(0, 0)];
    assert_eq!(r_cell.symbol(), "R");
    assert_eq!(r_cell.style().fg.unwrap(), Color::Indexed(196));
}

#[test]
fn test_snapshot_rgb_color() {
    let mut parser = vt100::Parser::new(3, 20, 0);
    // True color: ESC[38;2;255;128;0m (orange)
    parser.process(b"\x1b[38;2;255;128;0mOrange\x1b[0m");

    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    render_screen(parser.screen(), &mut buf, area);

    let o_cell = &buf[(0, 0)];
    assert_eq!(o_cell.symbol(), "O");
    assert_eq!(o_cell.style().fg.unwrap(), Color::Rgb(255, 128, 0));
}
