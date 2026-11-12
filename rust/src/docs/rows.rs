//! The text of a documentation page as Neovim buffer lines, free of any engine types.
//!
//! `docs.rs` reads the text of every visual line of the page's `RichTextLabel` and hands it
//! to [`rows_from_lines`], which decides what the Neovim buffer holds. Keeping this separate
//! lets the rule be unit tested without an editor (`cargo test`).

/// One line of the Neovim document buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The buffer line's text (no line break, no trailing whitespace).
    pub text: String,
    /// The main-frame visual line of the label this row is drawn in. Only the label's own
    /// line numbers are used to scroll the page; nothing is measured inside a line.
    pub line: usize,
}

/// Turns the text of every main-frame visual line into buffer rows.
///
/// `RichTextLabel` only reports lines of its *main frame*. A table (which is how Godot's
/// reference pages draw a `[codeblock]`) is one object inside one such line, but the text
/// it contains still belongs to that line's character range, line breaks included. So:
///
/// * a line's text normally has no line break (except the single one that ends its
///   paragraph, which is dropped) and becomes one row;
/// * a line whose text still contains line breaks is a code block, and becomes one row per
///   code line, so Neovim sees the code block as the several lines it really is.
///
/// Leading whitespace is kept (code indentation); trailing whitespace is dropped (a soft
/// wrap leaves a space behind).
pub fn rows_from_lines<S: AsRef<str>>(lines: &[S]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(lines.len());
    for (line, raw) in lines.iter().enumerate() {
        let raw: String = raw.as_ref().chars().filter(|c| *c != '\r').collect();
        // The paragraph's own terminator belongs to the line; only that one is dropped, so a
        // blank row inside a code block survives.
        let text = raw.strip_suffix('\n').unwrap_or(&raw);
        for part in text.split('\n') {
            rows.push(Row {
                text: part.trim_end().to_string(),
                line,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_lines_become_one_row_each() {
        let rows = rows_from_lines(&["Hello world \n", "second line\n", ""]);
        let text: Vec<_> = rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, ["Hello world", "second line", ""]);
        assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn leading_whitespace_is_kept() {
        let rows = rows_from_lines(&["    indented  \n"]);
        assert_eq!(rows[0].text, "    indented");
    }

    #[test]
    fn a_code_block_becomes_one_row_per_code_line() {
        let rows = rows_from_lines(&["Intro\n", "var a = 1\n\tprint(a)\nend\n", "Outro"]);
        let text: Vec<_> = rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, ["Intro", "var a = 1", "\tprint(a)", "end", "Outro"]);
        assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [0, 1, 1, 1, 2]);
    }

    #[test]
    fn a_blank_row_inside_a_code_block_survives() {
        let rows = rows_from_lines(&["a\n\nb\n"]);
        let text: Vec<_> = rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, ["a", "", "b"]);
    }

    #[test]
    fn carriage_returns_are_ignored() {
        let rows = rows_from_lines(&["a\r\nb\r\n"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].text, "b");
    }
}
