//! Integration with Godot's native class reference viewer.
//!
//! Godot owns documentation search (F1), the class database, symbol lookup and the
//! reference pages themselves (`EditorHelp`). None of that is re-implemented here.
//! Every documentation action in the editor (F1 search, "Open Documentation" in the
//! scene tree / inspector, links inside a reference page, `ScriptEditor.goto_help`)
//! ends in the built-in Script Editor creating an `EditorHelp` tab.
//!
//! [`VimdowDocs`] watches for that tab and *re-parents* the native `EditorHelp` node
//! into a transparent layer inside VimDow, then flips the main screen back to VimDow.
//!
//! # Documents
//! VimDow's documents are Neovim buffers (the editor is a Neovim UI), so a reference
//! page is registered as a regular, listed Neovim buffer (`godot-doc://Node`, see
//! `lua/vimdow/docs.lua`). It is switched with the usual buffer, window and tab
//! commands. This type only places each native page over the Neovim window that is
//! currently showing its buffer ([`VimdowDocs::apply_layout`]); there is no separate
//! navigation UI.
//!
//! # The page's text in Neovim
//! The buffer's lines are the text of the rendered page (one line per visual line of the
//! `RichTextLabel`, and one per code line of a code block, see [`page_rows`]), mirrored into
//! Neovim whenever the page content or its wrapping changes
//! ([`VimdowDocs::page_text_changed`]). Neovim motions, searches and buffer commands
//! therefore work on the page's text, and the page scrolls to keep the line Neovim's cursor
//! is on in view ([`reveal_line`]).
//!
//! # There is no cursor on a documentation page
//! The page is drawn by Godot's `RichTextLabel`, which cannot report where a character is:
//! it has no "rectangle of character n", knows nothing about the font of a run of text, and
//! draws a `[codeblock]` as a table whose inner lines it does not expose. `EditorHelp`
//! itself is not available to extensions, so VimDow cannot draw the page instead. A cursor
//! cell would have to be *guessed* from font widths and derived padding, and a guessed
//! cursor is wrong on mixed-font lines and in code blocks. So the page shows no cursor at
//! all. Everything the label does natively (mouse wheel, clicking links, selecting text) is
//! untouched. The only layout facts used here are the ones the label really reports: its
//! line count and the top and bottom of each of its visual lines.
//!
//! # Public API used
//! * `EditorInterface.get_script_editor()` / `ScriptEditor.goto_help(topic)`
//! * `EditorInterface.set_main_screen_editor(name)`
//! * `SceneTree.node_added`
//! * `ClassDB` (only to turn a word under the cursor into a `goto_help` topic)
//!
//! # Not public API (limitations)
//! `EditorHelp` is an editor-internal C++ class. It is not exposed to GDExtension, so
//! it cannot be instantiated or queried by VimDow; it can only be found in the scene
//! tree by class name and moved. See the "Limitations" section of the README.

use godot::classes::control::{FocusMode, MouseFilter};
use godot::classes::{
    ClassDb, ColorRect, Control, EditorInterface, EditorSettings, IControl, Node, Range,
    RichTextLabel, ScriptEditor,
};
use godot::prelude::*;

mod rows;

use rows::{Row, rows_from_lines};

/// Native class name of Godot's class reference page node.
const HELP_CLASS: &str = "EditorHelp";

fn script_editor() -> Option<Gd<ScriptEditor>> {
    EditorInterface::singleton().get_script_editor()
}

fn is_help_page(node: &Gd<Node>) -> bool {
    node.get_class().to_string() == HELP_CLASS
}

/// Collects every `EditorHelp` node below `root` (does not descend into them).
fn collect_help_pages(root: &Gd<Node>, out: &mut Vec<Gd<Node>>) {
    for child in root.get_children().iter_shared() {
        if is_help_page(&child) {
            out.push(child);
        } else {
            collect_help_pages(&child, out);
        }
    }
}

/// True if `member` is declared by `cls` itself (inherited members are excluded,
/// because the reference page of the declaring class is the one that documents it).
fn has_named(list: &Array<VarDictionary>, member: &StringName) -> bool {
    list.iter_shared().any(|entry| {
        entry
            .get("name")
            .and_then(|v| v.try_to::<StringName>().ok())
            .is_some_and(|n| &n == member)
    })
}

/// Returns the `ScriptEditor.goto_help` topic prefix for a member declared on `cls`.
fn declared_member_kind(
    db: &Gd<ClassDb>,
    cls: &StringName,
    member: &StringName,
) -> Option<&'static str> {
    if has_named(&db.class_get_property_list_ex(cls).no_inheritance(true).done(), member) {
        return Some("class_property");
    }
    if has_named(&db.class_get_method_list_ex(cls).no_inheritance(true).done(), member) {
        return Some("class_method");
    }
    if has_named(&db.class_get_signal_list_ex(cls).no_inheritance(true).done(), member) {
        return Some("class_signal");
    }
    let constants = db
        .class_get_integer_constant_list_ex(cls)
        .no_inheritance(true)
        .done();
    let member_str = member.to_string();
    if constants.as_slice().iter().any(|c| c.to_string() == member_str) {
        return Some("class_constant");
    }
    None
}

/// Maps a word to a `ScriptEditor.goto_help` topic, validated against `ClassDB`.
///
/// `contexts` are candidate classes in priority order (e.g. the receiver type reported
/// by the GDScript language server, then the script's `extends` class). Members are
/// resolved by walking each candidate's inheritance chain up to the declaring class.
/// Returns `None` when `ClassDB` knows nothing about the word, so no bogus topic is
/// ever sent to Godot.
pub fn topic_for(word: &str, contexts: &[String]) -> Option<String> {
    if word.is_empty() {
        return None;
    }
    let db = ClassDb::singleton();
    let word_name = StringName::from(word);

    if db.class_exists(&word_name) {
        return Some(format!("class_name:{word}"));
    }

    for ctx in contexts {
        let mut cls = StringName::from(ctx.as_str());
        if !db.class_exists(&cls) {
            continue;
        }
        while !cls.is_empty() {
            if let Some(kind) = declared_member_kind(&db, &cls, &word_name) {
                return Some(format!("{kind}:{cls}:{word}"));
            }
            cls = db.get_parent_class(&cls);
        }
    }
    None
}

/// A native page adopted into VimDow together with its opaque backdrop.
struct Page {
    /// Class-reference name (e.g. `Node2D`); also the Neovim buffer's document name.
    name: String,
    node: Gd<Node>,
    /// `EditorHelp` has a transparent background (the Script Editor paints it), so
    /// without this the Neovim grid underneath shows through the page.
    backdrop: Gd<ColorRect>,
    /// The page's `RichTextLabel`, found once its content has been created.
    label: Option<Gd<RichTextLabel>>,
    /// (visual line count, content height) last mirrored into the Neovim buffer.
    synced: (i64, i64),
    /// The rows last mirrored into the Neovim buffer: buffer line `n` is `rows[n]`.
    rows: Vec<Row>,
    /// Neovim's cursor line (= row) in this page, 0-based; -1 when unknown. It is only used
    /// to scroll the page; nothing is drawn for it.
    line: i64,
}

/// Layer that places Godot's native `EditorHelp` pages over the Neovim windows
/// showing their document buffers.
///
/// Add it as a child of `VimdowEditor`, then call [`VimdowDocs::attach`].
#[derive(GodotClass)]
#[class(tool, base=Control, init)]
pub struct VimdowDocs {
    base: Base<Control>,
    /// Name of VimDow's main screen tab, used to switch back after Godot jumps to "Script".
    main_screen_name: GString,
    pages: Vec<Page>,
    attached: bool,
}

#[godot_api]
impl VimdowDocs {
    /// Emitted when a native page has been adopted (or replaced). VimDow answers by
    /// opening/focusing the matching Neovim document buffer.
    #[signal]
    fn page_adopted(name: String);

    /// Emitted when a page's rendered lines changed (content or wrapping); `lines` are the
    /// new contents of the Neovim document buffer (one line per visual line, one per code
    /// line of a code block).
    #[signal]
    fn page_text_changed(name: String, lines: PackedStringArray);

    /// Starts watching for native documentation pages. `main_screen_name` is the name of
    /// VimDow's main screen tab (the plugin name).
    #[func]
    fn attach(&mut self, main_screen_name: GString) {
        self.main_screen_name = main_screen_name;
        if self.attached {
            return;
        }
        if !self.base().is_inside_tree() {
            godot_warn!("VimdowDocs.attach: node is not inside the scene tree");
            return;
        }
        let mut tree = self.base().get_tree();
        let cb = Callable::from_object_method(&self.to_gd(), "on_node_added");
        tree.connect("node_added", &cb);
        self.attached = true;
    }

    /// Stops watching and releases every adopted page.
    #[func]
    fn detach(&mut self) {
        if self.attached {
            if self.base().is_inside_tree() {
                let mut tree = self.base().get_tree();
                let cb = Callable::from_object_method(&self.to_gd(), "on_node_added");
                if tree.is_connected("node_added", &cb) {
                    tree.disconnect("node_added", &cb);
                }
            }
            self.attached = false;
        }
        for page in std::mem::take(&mut self.pages) {
            discard_page(page);
        }
    }

    /// Opens a documentation topic through Godot's own `ScriptEditor.goto_help`.
    /// Topic syntax is Godot's: `class_name:Node`, `class_property:Node2D:position`,
    /// `class_method:Node:add_child`, `class_signal:...`, `class_constant:...`.
    #[func]
    fn open_topic(&mut self, topic: GString) -> bool {
        let Some(mut se) = script_editor() else {
            return false;
        };
        se.goto_help(&topic);
        // If the page already existed as a Script Editor tab no node is added, so
        // look for it after the editor has finished switching tabs.
        self.base_mut().call_deferred("adopt_existing", &[]);
        true
    }

    /// Resolves `word` with `ClassDB` (see [`topic_for`]) and opens it. Returns false if
    /// the word is not a known native symbol.
    #[func]
    fn open_symbol(&mut self, word: GString, contexts: PackedStringArray) -> bool {
        let contexts: Vec<String> = contexts.as_slice().iter().map(|s| s.to_string()).collect();
        match topic_for(&word.to_string(), &contexts) {
            Some(topic) => self.open_topic(GString::from(topic.as_str())),
            None => false,
        }
    }

    /// Positions pages over the Neovim windows that currently show their buffers.
    ///
    /// `entries` is an array of dictionaries `{name, row, col, width, height, line}`: the
    /// window rectangle in Neovim grid cells and its cursor line (1-based). `cell` is the
    /// size of one cell in pixels and `origin` the pixel position of the Neovim grid
    /// relative to this layer. Pages without an entry (their buffer is not displayed) are
    /// hidden but kept alive, like a hidden buffer.
    #[func]
    fn apply_layout(&mut self, entries: VarArray, cell: Vector2, origin: Vector2) {
        let mut placed: Vec<String> = vec![];
        for entry in entries.iter_shared() {
            let Ok(d) = entry.try_to::<VarDictionary>() else {
                continue;
            };
            let Some(name) = d.get("name").and_then(|n| n.try_to::<String>().ok()) else {
                continue;
            };
            let (row, col) = (cell_value(&d, "row"), cell_value(&d, "col"));
            let (w, h) = (cell_value(&d, "width"), cell_value(&d, "height"));
            let line = cell_value(&d, "line") as i64 - 1;
            for page in self.pages.iter_mut() {
                if page.name != name || !page.node.is_instance_valid() {
                    continue;
                }
                let pos = origin + Vector2::new(col * cell.x, row * cell.y);
                let size = Vector2::new(w * cell.x, h * cell.y);
                page.backdrop.set_position(pos);
                page.backdrop.set_size(size);
                page.backdrop.set_visible(true);
                page.node.set("position", &pos.to_variant());
                page.node.set("size", &size.to_variant());
                page.node.set("visible", &true.to_variant());
                if line != page.line {
                    page.line = line;
                    let target = usize::try_from(line)
                        .ok()
                        .and_then(|n| page.rows.get(n))
                        .map(|row| row.line);
                    if let (Some(label), Some(target)) = (page.label.as_ref(), target) {
                        reveal_line(label, target);
                    }
                }
                placed.push(name.clone());
            }
        }
        for page in self.pages.iter_mut() {
            if !placed.contains(&page.name) && page.node.is_instance_valid() {
                page.node.set("visible", &false.to_variant());
                page.backdrop.set_visible(false);
            }
        }
    }

    /// Frees the page for a document buffer that was closed in Neovim.
    #[func]
    fn close_page(&mut self, name: GString) {
        let name = name.to_string();
        if let Some(idx) = self.pages.iter().position(|p| p.name == name) {
            discard_page(self.pages.remove(idx));
        }
    }

    #[func]
    fn on_node_added(&mut self, node: Gd<Node>) {
        if !is_help_page(&node) {
            return;
        }
        let Some(se) = script_editor() else {
            return;
        };
        if !se.is_ancestor_of(&node) {
            return; // e.g. our own re-parenting
        }
        // Deferred: let ScriptEditor finish its own bookkeeping for the new tab first.
        self.base_mut()
            .call_deferred("adopt", &[node.to_variant()]);
    }

    #[func]
    fn adopt_existing(&mut self) {
        let Some(se) = script_editor() else {
            return;
        };
        let mut found = vec![];
        collect_help_pages(&se.upcast::<Node>(), &mut found);
        for page in found {
            // TabContainer only keeps the selected tab visible.
            if page.get("visible").try_to::<bool>().unwrap_or(false) {
                self.adopt(page);
            }
        }
    }

    #[func]
    fn adopt(&mut self, mut page: Gd<Node>) {
        if !page.is_instance_valid() {
            return;
        }
        let Some(mut parent) = page.get_parent() else {
            return;
        };
        let name = page.get_name().to_string();
        parent.remove_child(&page);

        // One document per class reference: the freshly created page is already scrolled
        // to the requested member, so it replaces the older page of the same class.
        // (Removed from the tree first so the new page keeps its node name.)
        if let Some(idx) = self.pages.iter().position(|p| p.name == name) {
            discard_page(self.pages.remove(idx));
        }

        let mut backdrop = ColorRect::new_alloc();
        backdrop.set_color(backdrop_color());
        backdrop.set_mouse_filter(MouseFilter::IGNORE);
        backdrop.set_visible(false);
        self.base_mut().add_child(&backdrop);
        self.base_mut().add_child(&page);

        // Hidden until the Neovim window layout arrives, to avoid a misplaced flash.
        page.set("visible", &false.to_variant());
        // Keys stay with Neovim: clicking a link must not move keyboard focus.
        page.set("focus_mode", &(FocusMode::NONE.ord() as i64).to_variant());
        clear_focus(&page);
        self.pages.push(Page {
            name: name.clone(),
            node: page,
            backdrop,
            label: None,
            synced: (0, 0),
            rows: vec![],
            line: -1,
        });

        // Godot switches to the Script main screen when opening docs; go back to VimDow.
        if !self.main_screen_name.is_empty() {
            EditorInterface::singleton().set_main_screen_editor(&self.main_screen_name);
        }
        self.signals().page_adopted().emit(name);
    }
}

#[godot_api]
impl IControl for VimdowDocs {
    fn ready(&mut self) {
        // Transparent layer: only the pages themselves take mouse input.
        self.base_mut().set_mouse_filter(MouseFilter::IGNORE);
        self.base_mut().set_clip_contents(true);
    }

    fn process(&mut self, _delta: f32) {
        let mut changed: Vec<(String, PackedStringArray)> = vec![];
        for page in self.pages.iter_mut() {
            if !page.node.is_instance_valid() {
                continue;
            }
            if page.label.is_none() {
                // The page builds its content after being created; wait for it.
                page.label = find_rich_text(&page.node);
            }
            let Some(label) = page.label.as_ref() else {
                continue;
            };
            if !label.is_instance_valid() {
                page.label = None;
                continue;
            }
            // Mirror the rendered lines into the Neovim buffer when they change (new
            // content, or the window was resized and the text re-wrapped).
            let marker = (
                i64::from(line_count(label)),
                i64::from(label.get_content_height()),
            );
            if marker.0 > 0 && marker != page.synced {
                page.synced = marker;
                page.rows = page_rows(label);
                let lines: Vec<GString> =
                    page.rows.iter().map(|r| GString::from(r.text.as_str())).collect();
                changed.push((page.name.clone(), PackedStringArray::from(lines.as_slice())));
            }
        }
        for (name, lines) in changed {
            self.signals().page_text_changed().emit(name, &lines);
        }
    }
}

fn discard_page(page: Page) {
    for mut node in [page.backdrop.upcast::<Node>(), page.node] {
        if !node.is_instance_valid() {
            continue;
        }
        if let Some(mut parent) = node.get_parent() {
            parent.remove_child(&node);
        }
        node.queue_free();
    }
}

/// Opaque background for pages, matching the editor base colour VimDow already uses.
fn backdrop_color() -> Color {
    let mut ei = EditorInterface::singleton();
    // Called dynamically: whether the accessor returns an Option varies between versions.
    ei.call("get_editor_settings", &[])
        .try_to::<Gd<EditorSettings>>()
        .ok()
        .and_then(|es| es.get_setting("interface/theme/base_color").try_to::<Color>().ok())
        .unwrap_or(Color::from_rgb(0.16, 0.16, 0.16))
}

/// Reads a (possibly integer) number from a layout entry.
fn cell_value(d: &VarDictionary, key: &str) -> f32 {
    d.get(key)
        .and_then(|v| {
            v.try_to::<i64>()
                .map(|i| i as f32)
                .or_else(|_| v.try_to::<f64>().map(|f| f as f32))
                .ok()
        })
        .unwrap_or(0.0)
}

fn find_rich_text(root: &Gd<Node>) -> Option<Gd<RichTextLabel>> {
    for child in root.get_children().iter_shared() {
        if let Ok(label) = child.clone().try_cast::<RichTextLabel>() {
            return Some(label);
        }
        if let Some(label) = find_rich_text(&child) {
            return Some(label);
        }
    }
    None
}

fn clear_focus(root: &Gd<Node>) {
    for child in root.get_children().iter_shared() {
        if let Ok(mut c) = child.clone().try_cast::<Control>() {
            c.set_focus_mode(FocusMode::NONE);
        }
        clear_focus(&child);
    }
}

/// The label's vertical scroll bar, fetched dynamically (the accessor's exact Rust
/// signature varies between godot-rust versions).
fn v_scroll(label: &Gd<RichTextLabel>) -> Option<Gd<Range>> {
    let mut label = label.clone();
    label.call("get_v_scroll_bar", &[]).try_to::<Gd<Range>>().ok()
}

// The layout queries below validate the label's line cache, so depending on the bindings
// they are `&mut self` methods; working on a cheap clone of the handle works either way.

/// Number of visual (wrapped) lines of the page.
#[allow(unused_mut)]
fn line_count(label: &Gd<RichTextLabel>) -> i32 {
    let mut l = label.clone();
    l.get_line_count()
}

/// Content-space top of a visual line.
#[allow(unused_mut)]
fn line_top(label: &Gd<RichTextLabel>, line: i32) -> f32 {
    let mut l = label.clone();
    l.get_line_offset(line)
}

/// Content-space bottom of a visual line (= top of the next one).
fn line_bottom(label: &Gd<RichTextLabel>, line: i32) -> f32 {
    if line + 1 < line_count(label) {
        line_top(label, line + 1)
    } else {
        label.get_content_height() as f32
    }
}

/// The raw text of every main-frame visual line of the page, in order: the characters
/// from the line's first character to the first character of the next line, *line breaks
/// included* (see [`rows_from_lines`] for what a line break inside a line means).
///
/// Only the *start* of each line is taken from `RichTextLabel.get_line_range`, because that
/// is unambiguous whether or not the end is inclusive.
fn line_texts(label: &Gd<RichTextLabel>) -> Vec<String> {
    let parsed: Vec<char> = label.get_parsed_text().to_string().chars().collect();
    let count = line_count(label).max(0);

    let starts: Option<Vec<usize>> = if label.has_method("get_line_range") {
        let mut l = label.clone();
        (0..count)
            .map(|i| {
                l.call("get_line_range", &[i.to_variant()])
                    .try_to::<Vector2i>()
                    .ok()
                    .filter(|r| r.x >= 0 && (r.x as usize) <= parsed.len())
                    .map(|r| r.x as usize)
            })
            .collect()
    } else {
        None
    };

    match starts {
        // The starts must be non-decreasing, otherwise the API did not behave as assumed.
        Some(starts) if starts.windows(2).all(|w| w[0] <= w[1]) => (0..starts.len())
            .map(|i| {
                let end = starts.get(i + 1).copied().unwrap_or(parsed.len());
                parsed[starts[i]..end].iter().collect()
            })
            .collect(),
        _ => {
            // Fallback: ask for the line of every character (slower, but only 4.0 API).
            let mut out = vec![String::new(); count as usize];
            #[allow(unused_mut)]
            let mut l = label.clone();
            for (i, c) in parsed.iter().enumerate() {
                let line = l.get_character_line(i as i32);
                if let Some(slot) = usize::try_from(line).ok().and_then(|n| out.get_mut(n)) {
                    slot.push(*c);
                }
            }
            out
        }
    }
}

/// The rows of the page: what the Neovim buffer holds, one buffer line per row.
fn page_rows(label: &Gd<RichTextLabel>) -> Vec<Row> {
    rows_from_lines(&line_texts(label))
}

/// Scrolls the page just enough to bring the visual line `line` into view (like Neovim
/// keeping its cursor on screen). Only called when the cursor line changes, so mouse-wheel
/// scrolling is never fought. A code block is a single line to the label, so the block is
/// brought into view as a whole (its top, when it is taller than the page).
fn reveal_line(label: &Gd<RichTextLabel>, line: usize) {
    let Ok(line) = i32::try_from(line) else {
        return;
    };
    if line >= line_count(label) {
        return;
    }
    let (top, bottom) = (line_top(label, line), line_bottom(label, line));
    if bottom <= top {
        return; // not laid out yet
    }
    let Some(mut range) = v_scroll(label) else {
        return;
    };
    let (scroll, view_h) = (range.get_value() as f32, label.get_size().y);
    if top < scroll {
        range.set_value(f64::from(top));
    } else if bottom > scroll + view_h {
        range.set_value(f64::from((bottom - view_h).min(top)));
    }
}
