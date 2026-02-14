pub mod converter;

use vt100::Parser;

pub struct TerminalEmulator {
    parser: Parser,
}

impl TerminalEmulator {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Parser::new(rows, cols, 1000),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(rows, cols);
    }

    pub fn rows(&self) -> u16 {
        self.parser.screen().size().0
    }

    pub fn cols(&self) -> u16 {
        self.parser.screen().size().1
    }
}
