// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A terminal emulator that follows what a session prints.

use std::num::NonZeroU16;

use crate::process::PtySize;

const RESET_ATTRIBUTES: &str = "\u{1b}[m";
const LINE_BREAK: &str = "\r\n";
const BASIC_COLORS: u8 = 8;
const BRIGHT_COLORS: u8 = 16;

fn dimension(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).unwrap_or(NonZeroU16::MIN)
}

/// How a cell is drawn: its colours and text attributes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    foreground: vt100::Color,
    background: vt100::Color,
    is_bold: bool,
    is_dim: bool,
    is_italic: bool,
    is_underlined: bool,
    is_inverse: bool,
}

impl Style {
    fn of(cell: &vt100::Cell) -> Self {
        Self {
            foreground: cell.fgcolor(),
            background: cell.bgcolor(),
            is_bold: cell.bold(),
            is_dim: cell.dim(),
            is_italic: cell.italic(),
            is_underlined: cell.underline(),
            is_inverse: cell.inverse(),
        }
    }

    /// The escape sequence that sets this style from any other.
    fn escape(self) -> String {
        let mut codes = vec!["0".to_owned()];
        let flags = [
            (self.is_bold, "1"),
            (self.is_dim, "2"),
            (self.is_italic, "3"),
            (self.is_underlined, "4"),
            (self.is_inverse, "7"),
        ];
        codes.extend(
            flags
                .iter()
                .filter(|(is_set, _)| *is_set)
                .map(|(_, code)| (*code).to_owned()),
        );
        codes.extend(color_code(self.foreground, 30));
        codes.extend(color_code(self.background, 40));
        format!("\u{1b}[{}m", codes.join(";"))
    }
}

/// The SGR parameters of a colour; `base` is 30 for the text, 40 for the background.
fn color_code(color: vt100::Color, base: u8) -> Option<String> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) if index < BASIC_COLORS => Some((base + index).to_string()),
        vt100::Color::Idx(index) if index < BRIGHT_COLORS => Some((base + 60 + index - BASIC_COLORS).to_string()),
        vt100::Color::Idx(index) => Some(format!("{};5;{index}", base + 8)),
        vt100::Color::Rgb(red, green, blue) => Some(format!("{};2;{red};{green};{blue}", base + 8)),
    }
}

/// One row of the terminal, as text that can be printed again anywhere.
struct Row {
    /// What the row shows, with its colours, up to its last drawn cell.
    styled: String,
    plain: String,
    /// Whether the row runs on into the next one: both are one line, cut by the width of the terminal.
    is_wrapped: bool,
}

/// The row at the top of the view (`row` 0 while scrolled back) or any row of the screen.
fn read_row(screen: &vt100::Screen, row: u16, cols: u16) -> Row {
    let is_drawn = |cell: &vt100::Cell| cell.has_contents() || Style::of(cell) != Style::default();
    let width = (0..cols)
        .rev()
        .find(|col| screen.cell(row, *col).is_some_and(is_drawn))
        .map_or(0, |last| last + 1);
    let (mut styled, mut plain) = (String::new(), String::new());
    let mut style = Style::default();
    for cell in (0..width)
        .filter_map(|col| screen.cell(row, col))
        .filter(|cell| !cell.is_wide_continuation())
    {
        let cell_style = Style::of(cell);
        if cell_style != style {
            styled.push_str(&cell_style.escape());
            style = cell_style;
        }
        let contents = if cell.has_contents() { cell.contents() } else { " " };
        styled.push_str(contents);
        plain.push_str(contents);
    }
    if style != Style::default() {
        styled.push_str(RESET_ATTRIBUTES);
    }
    Row {
        styled,
        plain,
        is_wrapped: screen.row_wrapped(row),
    }
}

/// A terminal emulator fed with everything a session prints, at the size its pseudo-terminal has.
/// Replaying the raw output in a terminal of another size breaks it: a full-screen program redraws
/// in place with cursor moves counted for the width it printed at. The mirror hands a terminal
/// opened later the screen and its history as they stand, and rewraps them when the size changes.
pub struct TerminalMirror {
    parser: vt100::Parser,
    scrollback_lines: usize,
}

impl TerminalMirror {
    pub fn new(size: PtySize, scrollback_lines: usize) -> Self {
        Self {
            parser: vt100::Parser::new(dimension(size.rows), dimension(size.cols), scrollback_lines),
            scrollback_lines,
        }
    }

    pub fn write(&mut self, data: &[u8]) {
        self.parser.process(data);
    }

    fn size(&self) -> PtySize {
        let (rows, cols) = self.parser.screen().size();
        PtySize {
            cols: cols.get(),
            rows: rows.get(),
        }
    }

    /// Rows that scrolled off the screen, oldest first.
    fn history(&mut self) -> Vec<Row> {
        let cols = self.size().cols;
        let screen = self.parser.screen_mut();
        screen.set_scrollback(usize::MAX);
        let length = screen.scrollback();
        let rows = (1..=length)
            .rev()
            .map(|offset| {
                screen.set_scrollback(offset);
                read_row(screen, 0, cols)
            })
            .collect();
        screen.set_scrollback(0);
        rows
    }

    fn screen_rows(&self) -> Vec<Row> {
        let size = self.size();
        (0..size.rows)
            .map(|row| read_row(self.parser.screen(), row, size.cols))
            .collect()
    }

    /// Takes the size of the terminal it mirrors. A new width rewraps the history and the screen,
    /// as a terminal does: nothing printed is lost to a narrower window.
    pub fn resize(&mut self, size: PtySize) {
        let current = self.size();
        if current == size {
            return;
        }
        // A full-screen program on the alternate screen redraws itself: there is nothing to rewrap.
        if current.cols == size.cols || self.parser.screen().alternate_screen() {
            self.parser.set_size(dimension(size.rows), dimension(size.cols));
            return;
        }
        let replay = self.rewrappable();
        self.parser = vt100::Parser::new(dimension(size.rows), dimension(size.cols), self.scrollback_lines);
        self.parser.process(replay.as_bytes());
    }

    /// Everything the terminal holds, as lines a terminal of any width wraps by itself, with the
    /// cursor put back where it was.
    fn rewrappable(&mut self) -> String {
        let screen = self.parser.screen();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let modes = format!(
            "{}{}",
            screen.input_mode_formatted(),
            if screen.hide_cursor() { "\u{1b}[?25l" } else { "" }
        );
        let history = self.history();
        let cursor_index = history.len() + usize::from(cursor_row);
        let rows: Vec<Row> = history.into_iter().chain(self.screen_rows()).collect();
        let last_drawn = rows.iter().rposition(|row| !row.plain.trim().is_empty()).unwrap_or(0);
        let last = last_drawn.max(cursor_index);

        let mut output = String::new();
        for (index, row) in rows.iter().enumerate().take(last + 1) {
            output.push_str(&row.styled);
            if !row.is_wrapped && index < last {
                output.push_str(LINE_BREAK);
            }
        }
        if last > cursor_index {
            output.push_str(&format!("\u{1b}[{}A", last - cursor_index));
        }
        output.push('\r');
        if cursor_col > 0 {
            output.push_str(&format!("\u{1b}[{cursor_col}C"));
        }
        output.push_str(&modes);
        output
    }

    /// Escape sequences that redraw the history then the screen in a terminal of the same size.
    pub fn snapshot(&mut self) -> String {
        let rows = self.size().rows;
        let history = self.history();
        let mut output = String::new();
        if !history.is_empty() {
            let last = history.len() - 1;
            for (index, row) in history.iter().enumerate() {
                output.push_str(&row.styled);
                if !row.is_wrapped || index == last {
                    output.push_str(LINE_BREAK);
                }
            }
            // Pushes the history out of view: what follows clears the screen before redrawing it.
            output.push_str(&LINE_BREAK.repeat(usize::from(rows) - 1));
        }
        output.push_str(&self.parser.screen().state_formatted());
        output
    }

    /// The last `count` lines of the history and the screen as plain text, trailing blank lines left out.
    pub fn text(&mut self, count: usize) -> String {
        let mut lines: Vec<String> = self
            .history()
            .into_iter()
            .chain(self.screen_rows())
            .map(|row| row.plain)
            .collect();
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        let start = lines.len().saturating_sub(count);
        lines[start..]
            .iter()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: PtySize = PtySize { cols: 20, rows: 4 };

    fn mirror_of(output: &str) -> TerminalMirror {
        let mut mirror = TerminalMirror::new(SIZE, 100);
        mirror.write(output.as_bytes());
        mirror
    }

    #[test]
    fn reads_the_end_of_the_screen_and_its_history_as_text() {
        let mut mirror = mirror_of("one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\n");
        assert_eq!(mirror.text(3), "four\nfive\nsix");
        assert_eq!(mirror.text(50), "one\ntwo\nthree\nfour\nfive\nsix");
    }

    #[test]
    fn leaves_trailing_blank_lines_out_of_the_text() {
        assert_eq!(mirror_of("only\r\n\r\n").text(10), "only");
        assert_eq!(mirror_of("").text(10), "");
    }

    #[test]
    fn redraws_the_same_screen_and_history_in_another_terminal() {
        let mut mirror = mirror_of("one\r\ntwo\r\n\u{1b}[31mthree\u{1b}[m\r\nfour\r\nfive\r\nsix\r\nprompt> ");
        let snapshot = mirror.snapshot();
        let mut replica = mirror_of(&snapshot);
        assert_eq!(replica.text(50), mirror.text(50));
        assert_eq!(replica.parser.screen().contents(), mirror.parser.screen().contents());
        assert_eq!(
            replica.parser.screen().cursor_position(),
            mirror.parser.screen().cursor_position()
        );
    }

    #[test]
    fn keeps_the_colours_of_the_history() {
        let mut mirror = mirror_of("\u{1b}[1;31mred\u{1b}[m plain \u{1b}[38;5;200mpink\u{1b}[m\r\n\r\n\r\n\r\n\r\n");
        let snapshot = mirror.snapshot();
        assert!(
            snapshot.starts_with("\u{1b}[0;1;31mred\u{1b}[0m plain \u{1b}[0;38;5;200mpink\u{1b}[m\r\n"),
            "{snapshot:?}"
        );
    }

    #[test]
    fn keeps_a_program_drawn_in_place_as_it_stands() {
        // A full-screen redraw: clear, then text placed with cursor moves.
        let mirror = mirror_of("noise\r\n\u{1b}[2J\u{1b}[2;3Hbox\u{1b}[4;1Hstatus");
        let mut same = mirror_of("noise\r\n\u{1b}[2J\u{1b}[2;3Hbox\u{1b}[4;1Hstatus");
        let replica = mirror_of(&same.snapshot());
        assert_eq!(replica.parser.screen().contents(), mirror.parser.screen().contents());
    }

    #[test]
    fn rewraps_long_lines_when_the_terminal_gets_narrower_or_wider() {
        let line = "abcdefghijklmnopqrstuvwxyz0123";
        let mut mirror = mirror_of(&format!("first\r\n{line}\r\nprompt> "));
        mirror.resize(PtySize { cols: 10, rows: 4 });
        // Nothing is cut: the line now takes three rows, the first of which scrolled into the history.
        assert_eq!(mirror.text(50), "first\nabcdefghij\nklmnopqrst\nuvwxyz0123\nprompt>");
        assert_eq!(mirror.parser.screen().cursor_position(), (3, 8));

        mirror.resize(PtySize { cols: 40, rows: 4 });
        assert_eq!(mirror.text(50), format!("first\n{line}\nprompt>"));
        assert_eq!(mirror.parser.screen().cursor_position(), (2, 8));
        // What the session prints next lands after the prompt.
        mirror.write(b"ok");
        assert_eq!(mirror.text(1), "prompt> ok");
    }

    #[test]
    fn follows_a_resize_that_only_changes_the_height() {
        let mut mirror = mirror_of("hello");
        mirror.resize(PtySize { cols: 20, rows: 10 });
        assert_eq!(mirror.size(), PtySize { cols: 20, rows: 10 });
        assert_eq!(mirror.text(5), "hello");
    }
}
