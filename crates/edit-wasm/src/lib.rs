// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! WebAssembly entry points for the edit terminal editor.
//!
//! JavaScript drives the editor by calling:
//! * [`editor_init`]   – once, before anything else.
//! * [`editor_step`]   – on every keyboard/mouse/paste event (passes VT bytes).
//! * [`editor_resize`] – whenever the xterm.js terminal is resized.
//!
//! Each call returns a string of VT bytes that xterm.js should write to its
//! terminal so the display is updated.

// ── include the editor binary's source modules ────────────────────────────────
//
// These files live in `crates/edit/src/bin/edit/`.  They use `crate::` to
// refer to their siblings, so as long as we re-declare them all here under the
// same names they compile cleanly inside this crate.

#[path = "../../edit/src/bin/edit/apperr.rs"]
mod apperr;
#[path = "../../edit/src/bin/edit/documents.rs"]
mod documents;
#[path = "../../edit/src/bin/edit/draw_editor.rs"]
mod draw_editor;
#[path = "../../edit/src/bin/edit/draw_filepicker.rs"]
mod draw_filepicker;
#[path = "../../edit/src/bin/edit/draw_menubar.rs"]
mod draw_menubar;
#[path = "../../edit/src/bin/edit/draw_statusbar.rs"]
mod draw_statusbar;
#[path = "../../edit/src/bin/edit/localization.rs"]
mod localization;
#[path = "../../edit/src/bin/edit/settings.rs"]
mod settings;
#[path = "../../edit/src/bin/edit/state.rs"]
mod state;

// ── crate-level imports ───────────────────────────────────────────────────────

use std::borrow::Cow;
use std::cell::RefCell;
use std::time::Duration;

use draw_editor::*;
use draw_filepicker::*;
use draw_menubar::*;
use draw_statusbar::*;
use edit::framebuffer::IndexedColor;
use edit::helpers::*;
use edit::input::{self, kbmod, vk};
use edit::tui::*;
use edit::vt;
use edit::{base64, sys};
use localization::*;
use state::*;
use stdext::arena::{self, Arena, scratch_arena};
use stdext::arena_format;
use stdext::collections::BString;
use wasm_bindgen::prelude::*;

// ── constants ────────────────────────────────────────────────────────────────

/// Scratch arena size for WASM – much smaller than the native 128/512 MiB to
/// keep browser memory usage reasonable.
const SCRATCH_ARENA_CAPACITY: usize = 16 * MEBI;
/// Clipboard size threshold (same definition as in the native main.rs).
const LARGE_CLIPBOARD_THRESHOLD: usize = 128 * KIBI;

// ── global editor state ───────────────────────────────────────────────────────

struct EditorState {
    vt_parser: vt::Parser,
    input_parser: input::Parser,
    tui: Tui,
    state: state::State,
}

thread_local! {
    static EDITOR: RefCell<Option<EditorState>> = RefCell::new(None);
}

// ── public WASM API ───────────────────────────────────────────────────────────

/// Initialise the editor for a terminal with the given dimensions.
///
/// Must be called exactly once before any other function.
/// Returns the initial VT bytes to write to xterm.js.
#[wasm_bindgen]
pub fn editor_init(cols: u32, rows: u32) -> String {
    // One-time arena initialisation.  Panics if called twice, so guard it.
    EDITOR.with(|e| {
        if e.borrow().is_some() {
            return;
        }

        arena::init(SCRATCH_ARENA_CAPACITY).expect("failed to initialise arena");

        // Initialise platform abstraction layer and localisation.
        let _deinit = sys::init();
        localization::init();

        // Build editor state.
        let mut st = state::State::new().expect("failed to create editor state");

        // Gracefully ignore settings load failures (no filesystem in WASM).
        if let Err(err) = settings::Settings::reload() {
            st.add_error(err);
        }

        let vt_parser = vt::Parser::new();
        let input_parser = input::Parser::new();
        let tui = Tui::new().expect("failed to create Tui");

        *e.borrow_mut() = Some(EditorState { vt_parser, input_parser, tui, state: st });
    });

    // Store the terminal size and emit the VT mode-setup sequence that
    // xterm.js expects (alternative screen, mouse tracking, etc.).
    sys::wasm_set_size(cols as u16, rows as u16);

    sys::write_stdout(concat!(
        "\x1b[?1049h",       // Alternative Screen Buffer
        "\x1b[?1002;1006h",  // Cell Motion + SGR Mouse Mode
        "\x1b[?2004h",       // Bracketed Paste Mode
        "\x1b[?1036h",       // Meta sends escape
    ));

    // Set up TUI colours from defaults (no terminal query round-trip in WASM).
    EDITOR.with(|e| {
        if let Some(ed) = e.borrow_mut().as_mut() {
            let floater_bg = ed
                .tui
                .indexed_alpha(IndexedColor::Background, 2, 3)
                .oklab_blend(ed.tui.indexed_alpha(IndexedColor::Foreground, 1, 3));
            let floater_fg = ed.tui.contrasted(floater_bg);
            ed.state.menubar_color_bg = ed.tui.indexed(IndexedColor::Background).oklab_blend(
                ed.tui.indexed_alpha(IndexedColor::BrightBlue, 1, 2),
            );
            ed.state.menubar_color_fg = ed.tui.contrasted(ed.state.menubar_color_bg);
            ed.tui.setup_modifier_translations(ModifierTranslations {
                ctrl: loc(LocId::Ctrl),
                alt: loc(LocId::Alt),
                shift: loc(LocId::Shift),
            });
            ed.tui.set_floater_default_bg(floater_bg);
            ed.tui.set_floater_default_fg(floater_fg);
            ed.tui.set_modal_default_bg(floater_bg);
            ed.tui.set_modal_default_fg(floater_fg);
        }
    });

    // Inject the initial window-size VT sequence so the TUI knows the
    // terminal dimensions on the very first render.
    sys::wasm_set_size(cols as u16, rows as u16);
    sys::inject_window_size_into_stdin();

    step_internal("")
}

/// Feed VT-encoded input bytes (keyboard / mouse / paste) to the editor.
///
/// Returns VT bytes to write to xterm.js.
#[wasm_bindgen]
pub fn editor_step(input: &str) -> String {
    step_internal(input)
}

/// Notify the editor that the terminal has been resized.
///
/// Returns VT bytes to write to xterm.js.
#[wasm_bindgen]
pub fn editor_resize(cols: u32, rows: u32) -> String {
    sys::wasm_set_size(cols as u16, rows as u16);
    sys::inject_window_size_into_stdin();
    step_internal("")
}

// ── internal step logic ───────────────────────────────────────────────────────

fn step_internal(input: &str) -> String {
    EDITOR.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let Some(ed) = borrow.as_mut() else {
            return String::new();
        };

        // Push the caller's input into the WASM stdin buffer.
        sys::wasm_push_input(input);

        // Process a batch of input.
        {
            let scratch = scratch_arena(None);
            let read_timeout =
                ed.vt_parser.read_timeout().min(ed.tui.read_timeout()).min(Duration::ZERO);
            let Some(vt_input) = sys::read_stdin(&scratch, read_timeout) else {
                return sys::wasm_take_output();
            };

            let vt_iter = ed.vt_parser.parse(&vt_input);
            let mut input_iter = ed.input_parser.parse(vt_iter);

            while {
                let event = input_iter.next();
                let more = event.is_some();
                let mut ctx = ed.tui.create_context(event);
                draw(&mut ctx, &mut ed.state);
                more
            } {}
        }

        // Settle the layout (may take more than one pass).
        while ed.tui.needs_settling() {
            let mut ctx = ed.tui.create_context(None);
            draw(&mut ctx, &mut ed.state);
        }

        if ed.state.exit {
            // The user asked to quit; write the cleanup sequence and signal done.
            sys::write_stdout("\x1b[0 q\x1b[?25h\x1b]0;\x07\x1b[?1002;1006;2004l\x1b[?1049l");
            return sys::wasm_take_output();
        }

        // Render.
        {
            let scratch = scratch_arena(None);
            let mut output = ed.tui.render(&scratch);

            write_terminal_title(&scratch, &mut output, &mut ed.state);

            if ed.state.osc_clipboard_sync {
                write_osc_clipboard(&scratch, &mut output, &mut ed.tui, &mut ed.state);
            }

            sys::write_stdout(&output);
        }

        sys::wasm_take_output()
    })
}

// ── helper functions (adapted from crates/edit/src/bin/edit/main.rs) ─────────

fn draw(ctx: &mut Context, st: &mut state::State) {
    draw_menubar(ctx, st);
    draw_editor(ctx, st);
    draw_statusbar(ctx, st);

    if st.wants_close {
        draw_handle_wants_close(ctx, st);
    }
    if st.wants_exit {
        draw_handle_wants_exit(ctx, st);
    }
    if st.wants_goto {
        draw_goto_menu(ctx, st);
    }
    if st.wants_file_picker != StateFilePicker::None {
        draw_file_picker(ctx, st);
    }
    if st.wants_save {
        draw_handle_save(ctx, st);
    }
    if st.wants_language_picker {
        draw_dialog_language_change(ctx, st);
    }
    if st.wants_encoding_change != StateEncodingChange::None {
        draw_dialog_encoding_change(ctx, st);
    }
    if st.wants_go_to_file {
        draw_go_to_file(ctx, st);
    }
    if st.wants_about {
        draw_dialog_about(ctx, st);
    }
    if ctx.clipboard_ref().wants_host_sync() {
        draw_handle_clipboard_change(ctx, st);
    }
    if st.error_log_count != 0 {
        draw_error_log(ctx, st);
    }

    if let Some(key) = ctx.keyboard_input() {
        if key == kbmod::CTRL | vk::N {
            draw_add_untitled_document(ctx, st);
        } else if key == kbmod::CTRL | vk::O {
            st.wants_file_picker = StateFilePicker::Open;
        } else if key == kbmod::CTRL | vk::S {
            st.wants_save = true;
        } else if key == kbmod::CTRL_SHIFT | vk::S {
            st.wants_file_picker = StateFilePicker::SaveAs;
        } else if key == kbmod::CTRL | vk::W {
            st.wants_close = true;
        } else if key == kbmod::CTRL | vk::P {
            st.wants_go_to_file = true;
        } else if key == kbmod::CTRL | vk::Q {
            st.wants_exit = true;
        } else if key == kbmod::CTRL | vk::G {
            st.wants_goto = true;
        } else if key == kbmod::CTRL | vk::F
            && st.wants_search.kind != StateSearchKind::Disabled
        {
            st.wants_search.kind = StateSearchKind::Search;
            st.wants_search.focus = true;
        } else if key == kbmod::CTRL | vk::R
            && st.wants_search.kind != StateSearchKind::Disabled
        {
            st.wants_search.kind = StateSearchKind::Replace;
            st.wants_search.focus = true;
        } else if key == vk::F3 {
            search_execute(ctx, st, SearchAction::Search);
        } else {
            return;
        }
        ctx.needs_rerender();
        ctx.set_input_consumed();
    }
}

fn draw_handle_wants_exit(_ctx: &mut Context, st: &mut state::State) {
    while let Some(doc) = st.documents.active() {
        if doc.buffer.borrow().is_dirty() {
            st.wants_close = true;
            return;
        }
        st.documents.remove_active();
    }
    if st.documents.len() == 0 {
        st.exit = true;
    }
}

fn write_terminal_title<'a>(arena: &'a Arena, output: &mut BString<'a>, st: &mut state::State) {
    let (filename, dirty) = st
        .documents
        .active()
        .map_or(("", false), |d| (&d.filename, d.buffer.borrow().is_dirty()));

    if filename == st.osc_title_file_status.filename && dirty == st.osc_title_file_status.dirty {
        return;
    }

    output.push_str(arena, "\x1b]0;");
    if !filename.is_empty() {
        if dirty {
            output.push_str(arena, "● ");
        }
        output.push_str(arena, &sanitize_control_chars(filename));
        output.push_str(arena, " - ");
    }
    output.push_str(arena, "edit\x1b\\");

    st.osc_title_file_status.filename = filename.to_string();
    st.osc_title_file_status.dirty = dirty;
}

fn draw_handle_clipboard_change(ctx: &mut Context, st: &mut state::State) {
    let data_len = ctx.clipboard_ref().read().len();

    if st.osc_clipboard_always_send || data_len < LARGE_CLIPBOARD_THRESHOLD {
        ctx.clipboard_mut().mark_as_synchronized();
        st.osc_clipboard_sync = true;
        return;
    }

    let over_limit = data_len >= SCRATCH_ARENA_CAPACITY / 4;
    let mut done = None;

    ctx.modal_begin("warning", loc(LocId::WarningDialogTitle));
    {
        ctx.block_begin("description");
        ctx.attr_padding(Rect::three(1, 2, 1));

        if over_limit {
            ctx.label("line1", loc(LocId::LargeClipboardWarningLine1));
            ctx.attr_position(Position::Center);
            ctx.label("line2", loc(LocId::SuperLargeClipboardWarning));
            ctx.attr_position(Position::Center);
        } else {
            let label2 = {
                let template = loc(LocId::LargeClipboardWarningLine2);
                let size = arena_format!(ctx.arena(), "{}", MetricFormatter(data_len));
                let mut label = BString::empty();
                label.reserve(ctx.arena(), template.len() + size.len());
                label.push_str(ctx.arena(), template);
                label.replace_once_in_place(ctx.arena(), "{size}", &size);
                label
            };
            ctx.label("line1", loc(LocId::LargeClipboardWarningLine1));
            ctx.attr_position(Position::Center);
            ctx.label("line2", &label2);
            ctx.attr_position(Position::Center);
            ctx.label("line3", loc(LocId::LargeClipboardWarningLine3));
            ctx.attr_position(Position::Center);
        }
        ctx.block_end();

        ctx.table_begin("choices");
        ctx.inherit_focus();
        ctx.attr_padding(Rect::three(0, 2, 1));
        ctx.attr_position(Position::Center);
        ctx.table_set_cell_gap(Size { width: 2, height: 0 });
        {
            ctx.table_next_row();
            ctx.inherit_focus();

            if over_limit {
                if ctx.button("ok", loc(LocId::Ok), ButtonStyle::default()) {
                    done = Some(true);
                }
                ctx.inherit_focus();
            } else {
                if ctx.button("always", loc(LocId::Always), ButtonStyle::default()) {
                    st.osc_clipboard_always_send = true;
                    done = Some(true);
                }
                if ctx.button("yes", loc(LocId::Yes), ButtonStyle::default()) {
                    done = Some(true);
                }
                if data_len < 10 * LARGE_CLIPBOARD_THRESHOLD {
                    ctx.inherit_focus();
                }
                if ctx.button("no", loc(LocId::No), ButtonStyle::default()) {
                    done = Some(false);
                }
                if data_len >= 10 * LARGE_CLIPBOARD_THRESHOLD {
                    ctx.inherit_focus();
                }
            }
        }
        ctx.table_end();
    }
    if ctx.modal_end() {
        done = Some(false);
    }

    if let Some(sync) = done {
        st.osc_clipboard_sync = sync;
        ctx.clipboard_mut().mark_as_synchronized();
        ctx.needs_rerender();
    }
}

#[cold]
fn write_osc_clipboard<'a>(
    arena: &'a Arena,
    output: &mut BString<'a>,
    tui: &mut Tui,
    st: &mut state::State,
) {
    let clipboard = tui.clipboard_mut();
    let data = clipboard.read();

    if !data.is_empty() {
        output.reserve_exact(arena, base64::encode_len(data.len()) + 16);
        output.push_str(arena, "\x1b]52;c;");
        base64::encode(arena, output, data);
        output.push_str(arena, "\x1b\\");
    }

    st.osc_clipboard_sync = false;
}

fn sanitize_control_chars(text: &str) -> Cow<'_, str> {
    if let Some(off) = text.bytes().position(|b| (..0x20u8).contains(&b)) {
        let mut sanitized = text.to_string();
        let vec = unsafe { sanitized.as_bytes_mut() };
        for i in &mut vec[off..] {
            *i = if (..0x20u8).contains(i) { b'_' } else { *i }
        }
        Cow::Owned(sanitized)
    } else {
        Cow::Borrowed(text)
    }
}
