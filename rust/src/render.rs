mod highlights;

use godot::classes::{Control, IControl, ProjectSettings};
use godot::global::cos;
use godot::{obj::WithBaseField, prelude::*};
use std::f64::consts::TAU;

use highlights::{Highlighter, HlAttr};

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

fn column_slice(row: &str, start: usize, end: usize) -> String {
    assert!(start <= end);
    row.graphemes(true).skip(start).take(end - start).collect()
}

/// The text of the grid cell at column `x` of `row` (a space past the end of the row).
fn cell_text(row: &str, x: usize) -> String {
    row.graphemes(true).nth(x).unwrap_or(" ").to_string()
}

/// This is the View + Model in the MVC architecture of vimdow.
/// Its job is to store the text, and render them with the according highlights.
///
/// WARNING: VimdowWindows require a Highlighter node as one of their children to work.
#[derive(GodotClass)]
#[class(tool, init, base=Control)]
pub struct VimdowWindow {
    base: Base<Control>,

    grid_text: Rope,

    /// The current position of the cursor on the grid
    #[init(val = Vector2i {x: -1, y: -1})]
    #[var]
    cursor: Vector2i,

    /// The current mode_info data structure that neovim is in.
    #[export]
    current_mode: VarDictionary,

    /// The child highlighter
    #[init(node = "Highlighter")]
    highlighter: OnReady<Gd<Highlighter>>,
}

#[godot_api]
impl VimdowWindow {
    /// Returns the number of visible lines on the grid
    #[func]
    fn get_line_count(&self) -> i32 {
        self.grid_text.len_lines() as i32
    }

    /// Gets a line of text from a specified index.
    /// Will panic if the index is out of bounds.
    #[func]
    fn get_line(&self, i: i32) -> String {
        self.grid_text.line(i as usize).to_string()
    }

    /// Returns the size of the grid in cells
    #[func]
    fn get_grid_size(&self) -> Vector2i {
        let y = self.get_line_count();
        let x = self.get_line(0).len() as i32;
        Vector2i { x, y }
    }

    /// Sets a line of text on the grid to the provided `text` value.
    #[func]
    fn set_line(&mut self, i: i32, text: String) {
        let i = i as usize;
        let start = self.grid_text.line_to_char(i);
        let end = self.grid_text.line_to_char(i + 1);
        self.grid_text.remove(start..end);
        self.grid_text.insert(start, &text);
    }

    /// Sets the size of the grid in cells.
    /// Will also resize and reset the window's highlighter.
    #[func]
    fn set_grid_size(&mut self, width: i32, height: i32) {
        self.highlighter.bind_mut().set_hl_regions(
            (0..=height)
                .map(|_| {
                    let mut out = PackedInt32Array::new();
                    out.resize(width as usize);
                    out.fill(0);
                    out
                })
                .collect(),
        );

        let mut row = " ".repeat(width as usize);
        row.push('\n');
        let mut text = row.repeat(height as usize);
        assert_eq!(
            text.remove(text.len() - 1),
            '\n',
            "Should remove last newline"
        );
        self.grid_text = text.into();
    }

    /// Sets all text on the grid to whitespace, and clears the highlighter.
    #[func]
    fn clear(&mut self) {
        for i in 0..self.get_line_count() {
            let line = self.get_line(i).to_string();
            let new_line: String = line
                .chars()
                .map(|c| if c.is_whitespace() { c } else { ' ' })
                .collect();
            self.set_line(i, new_line);
        }

        self.highlighter.bind_mut().clear();
    }

    /// starts a redraw with the provided highlight data.
    #[func]
    fn flush(&mut self, hl: VarDictionary, current_mode: VarDictionary) {
        self.highlighter.bind_mut().set_hl_data(hl);
        self.current_mode = current_mode;
        self.base_mut().queue_redraw();
    }

    /// Reads a row of the grid and renders it to the screen
    fn draw_row(&mut self, row: i32) {
        let ignore_hl: bool = ProjectSettings::singleton()
            .get_setting("vimdow/debug/ignore_hl")
            .try_to()
            .unwrap_or(false);

        let regions = self.highlighter.bind().get_regions(row);
        if !ignore_hl {
            // drawing background colors
            for r in regions.iter() {
                let position = Vector2 {
                    x: r.attr.char_size.x * r.start_col as f32,
                    y: r.attr.char_size.y * row as f32,
                };
                let size = Vector2 {
                    x: r.attr.char_size.x * (r.end_col - r.start_col) as f32,
                    y: r.attr.char_size.y,
                };

                // drawing colored background
                self.base_mut()
                    .draw_rect_ex(Rect2 { position, size }, r.attr.background)
                    .filled(true)
                    .done();
            }
        }

        for r in regions.iter() {
            let text_position = Vector2 {
                x: r.start_col as f32 * r.attr.char_size.x,
                y: row as f32 * r.attr.char_size.y
                    + r.attr
                        .font
                        .get_ascent_ex()
                        .font_size(r.attr.font_size)
                        .done(),
            };

            let region_text = column_slice(&self.get_line(row), r.start_col, r.end_col);

            self.base_mut()
                .draw_string_ex(&r.attr.font, text_position, &region_text)
                .font_size(r.attr.font_size)
                .modulate(if ignore_hl {
                    Color::WHITE
                } else {
                    r.attr.foreground
                })
                .done();

            let line_width = r.attr.font_size as f32 * 0.1;
            let l = r.len();
            let desc = r.attr.font.get_descent() as f64;
            if r.attr.undercurl {
                let amplitude = desc / 2.0;
                const STEPS_PER_CYCLE: usize = 8;
                let total_span = l * STEPS_PER_CYCLE;
                let mut points = PackedVector2Array::new();
                for i in 0..total_span {
                    let t = i as f64 / total_span as f64;
                    let y = text_position.y as f64
                        + amplitude
                        + -cos(l as f64 * t * TAU) * amplitude as f64;
                    let p = Vector2::new(
                        text_position.x + t as f32 * l as f32 * r.attr.char_size.x,
                        y as f32,
                    );
                    points.push(p);
                }
                self.base_mut()
                    .draw_polyline_ex(&points, r.attr.special)
                    .width(line_width)
                    .done();
            }

            let desc = r
                .attr
                .font
                .get_descent_ex()
                .font_size(r.attr.font_size)
                .done();

            if r.attr.strikethrough {
                self.draw_underline(text_position, l as f32, -desc / 2.0, &r.attr);
            }

            if r.attr.underline || r.attr.url {
                self.draw_underline(text_position, l as f32, desc, &r.attr);
            }

            if r.attr.underdouble {
                self.draw_underline(text_position, l as f32, desc, &r.attr);
                let next_text_position =
                    Vector2::new(text_position.x, text_position.y - r.attr.char_size.y * 0.2);
                self.draw_underline(next_text_position, l as f32, desc, &r.attr);
            }

            if r.attr.underdashed {
                self.draw_segmented_underline(text_position, 0.33, 2, l, &r.attr);
            }

            if r.attr.underdotted {
                self.draw_segmented_underline(text_position, 0.15, 3, l, &r.attr);
            }
        }

        // The cursor is a single cell: draw it once, on top of all of the row's regions.
        // (It used to be drawn from inside the region loop with an inclusive end column,
        // so every region touching the cursor cell drew its own copy.)
        if row == self.cursor.y {
            self.draw_cursor();
        }
    }

    fn draw_underline(
        &mut self,
        text_position: Vector2,
        region_len: f32,
        desc: f32,
        attr: &HlAttr,
    ) {
        let line = PackedVector2Array::from([
            Vector2::new(text_position.x, text_position.y + desc as f32),
            Vector2::new(
                text_position.x + region_len * attr.char_size.x,
                text_position.y + desc as f32,
            ),
        ]);
        self.base_mut()
            .draw_polyline_ex(&line, attr.special)
            .width(2.0)
            .done();
    }

    /// draws an underline made of mini lines.
    /// `segment_len` is the length of a segment in pixels.
    /// `segments_per_char` is an integer for how many segments should be fit into
    /// a segment. `region_len` is the length of the text region in cells.
    fn draw_segmented_underline(
        &mut self,
        text_position: Vector2,
        segment_len: f32,
        segments_per_char: i32,
        region_len: usize,
        attr: &HlAttr,
    ) {
        let desc = attr.font.get_descent_ex().font_size(attr.font_size).done();
        let y = text_position.y + desc as f32;
        for i in 0..region_len {
            let left = Vector2::new(text_position.x + i as f32 * attr.char_size.x, y);
            let right = Vector2::new(left.x + attr.char_size.x * segment_len, y);

            for j in 0..segments_per_char {
                let offset = j as f32 / segments_per_char as f32 * attr.char_size.x;
                let left = Vector2::new(left.x + offset, left.y);
                let right = Vector2::new(right.x + offset, right.y);
                self.base_mut().draw_line(left, right, attr.special);
            }
        }
    }

    /// Reads an integer entry of the current `mode_info` (see `:h ui-event-mode_info_set`).
    fn mode_int(&self, key: &str) -> Option<i32> {
        self.current_mode.get(key).and_then(|v| v.try_to::<i32>().ok())
    }

    /// Draws the cursor for the current mode: a single cell at the cursor position, shaped
    /// by `cursor_shape` / `cell_percentage` and coloured by `attr_id`.
    fn draw_cursor(&mut self) {
        // The cursor can be outside the grid (not yet reported, or mid-resize).
        let (cx, cy) = (self.cursor.x, self.cursor.y);
        if cx < 0 || cy < 0 || cy >= self.get_line_count() {
            return;
        }
        let Some(attr) = self.highlighter.bind().get_cursor_attr(&self.cursor) else {
            return;
        };

        // Cursor colours. `attr_id` 0 means "swap the colours of the cell under the cursor";
        // otherwise the cursor has a highlight of its own (e.g. `guicursor=...-Cursor`).
        let (fill, text_color) = match self.mode_int("attr_id") {
            Some(id) if id > 0 && self.highlighter.bind().has_hl(id) => {
                let cursor_attr = self.highlighter.bind().get_hl_attr(id);
                (cursor_attr.background, cursor_attr.foreground)
            }
            _ => (attr.foreground, attr.background),
        };

        let cs = attr.char_size;
        let position = Vector2::new(cx as f32, cy as f32) * cs;
        let percentage = self.mode_int("cell_percentage").unwrap_or(25).clamp(1, 100) as f32 / 100.0;
        let shape = self
            .current_mode
            .get("cursor_shape")
            .map(|v| v.to_string())
            .unwrap_or_default();

        match shape.as_str() {
            "vertical" => {
                // A bar at the left edge of the cell, `cell_percentage` of its width.
                let size = Vector2::new((cs.x * percentage).max(1.0), cs.y);
                self.base_mut()
                    .draw_rect_ex(Rect2 { position, size }, fill)
                    .filled(true)
                    .done();
            }
            "horizontal" => {
                // A bar along the bottom of the cell, `cell_percentage` of its height.
                let h = (cs.y * percentage).max(1.0);
                let position = Vector2::new(position.x, position.y + cs.y - h);
                self.base_mut()
                    .draw_rect_ex(
                        Rect2 {
                            position,
                            size: Vector2::new(cs.x, h),
                        },
                        fill,
                    )
                    .filled(true)
                    .done();
            }
            // "block", and anything a future Neovim adds: a full cell with the character
            // redrawn in the cursor's text colour.
            _ => {
                self.base_mut()
                    .draw_rect_ex(Rect2 { position, size: cs }, fill)
                    .filled(true)
                    .done();

                let region_text = cell_text(&self.get_line(cy), cx as usize);
                self.base_mut()
                    .draw_string_ex(
                        &attr.font,
                        Vector2 {
                            x: position.x,
                            y: position.y
                                + attr.font.get_ascent_ex().font_size(attr.font_size).done(),
                        },
                        &region_text,
                    )
                    .font_size(attr.font_size)
                    .modulate(text_color)
                    .done();
            }
        }
    }
}

#[godot_api]
impl IControl for VimdowWindow {
    fn draw(&mut self) {
        for i in 0..self.get_line_count() {
            self.draw_row(i);
        }
    }
}
