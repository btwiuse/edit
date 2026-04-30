// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! WASM-specific platform code.
//!
//! Provides the same public API as the unix/windows sys modules but backed by
//! in-memory buffers instead of real OS primitives.  JavaScript drives the
//! editor by pushing VT input and pulling VT output through the helpers at the
//! bottom of this file.

use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::ptr::NonNull;
use std::time;
use std::{io, mem};

use stdext::arena::Arena;
use stdext::collections::{BString, BVec};

// ── per-call I/O buffers ──────────────────────────────────────────────────────

thread_local! {
    /// VT bytes queued to be consumed by the next `read_stdin` call.
    static STDIN_BUF: RefCell<String> = RefCell::new(String::new());
    /// VT bytes produced by `write_stdout` and waiting to be sent to xterm.js.
    static STDOUT_BUF: RefCell<String> = RefCell::new(String::new());
    /// Current terminal size (cols, rows).
    static TERMINAL_SIZE: RefCell<(u16, u16)> = RefCell::new((80, 24));
    /// Whether a synthetic resize should be injected into the next read.
    static INJECT_RESIZE: RefCell<bool> = RefCell::new(false);
}

// ── public sys API ────────────────────────────────────────────────────────────

pub struct Deinit;

impl Drop for Deinit {
    fn drop(&mut self) {}
}

pub fn init() -> Deinit {
    Deinit
}

/// In a browser there is no redirected stdin; always returns `None`.
pub fn reopen_stdin_if_redirected() -> io::Result<Option<File>> {
    Ok(None)
}

/// No-op in WASM: xterm.js handles raw-mode.
pub fn switch_modes() -> io::Result<()> {
    Ok(())
}

pub fn inject_window_size_into_stdin() {
    INJECT_RESIZE.with(|f| *f.borrow_mut() = true);
}

/// Reads pending VT input.  Returns `Some("")` (empty) when there is no input
/// yet (i.e. the call acts as a non-blocking read regardless of `timeout`).
pub fn read_stdin(arena: &Arena, _timeout: time::Duration) -> Option<BString<'_>> {
    let should_resize = INJECT_RESIZE.with(|f| {
        let v = *f.borrow();
        *f.borrow_mut() = false;
        v
    });

    let mut result = STDIN_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        let s = BString::from_str(arena, &buf);
        buf.clear();
        s
    });

    if should_resize {
        let (cols, rows) = TERMINAL_SIZE.with(|s| *s.borrow());
        if cols > 0 && rows > 0 {
            let resize_seq = BString::from_str(arena, &format!("\x1b[8;{rows};{cols}t"));
            // Prepend the resize sequence so it is processed first.
            let mut combined = resize_seq;
            combined.push_str(arena, &result);
            result = combined;
        }
    }

    Some(result)
}

pub fn write_stdout(text: &str) {
    if text.is_empty() {
        return;
    }
    STDOUT_BUF.with(|buf| buf.borrow_mut().push_str(text));
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileId(u64);

/// Returns a path-hash–based file identifier (no real filesystem access).
pub fn file_id(_file: Option<&File>, path: &Path) -> io::Result<FileId> {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    Ok(FileId(hasher.finish()))
}

pub struct LibIcu {
    pub libicuuc: NonNull<c_void>,
    pub libicui18n: NonNull<c_void>,
}

/// ICU dynamic loading is not supported in WASM; always returns an error.
pub fn load_icu() -> io::Result<LibIcu> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "ICU not available in WASM"))
}

/// Stub: never called because `load_icu` always fails.
///
/// # Safety
///
/// This function is inherently unsafe because it transmutes an arbitrary symbol
/// address.  However it is never called in WASM.
pub unsafe fn get_proc_address<T>(
    _handle: NonNull<c_void>,
    _name: *const c_char,
) -> io::Result<T> {
    let _ = mem::size_of::<T>(); // suppress unused generic warning
    Err(io::Error::new(io::ErrorKind::Unsupported, "ICU not available in WASM"))
}

/// Returns an empty language list; the editor will fall back to English.
pub fn preferred_languages(_arena: &Arena) -> BVec<'_, &'_ str> {
    BVec::empty()
}

/// WASM does not have a drive concept; returns an empty iterator.
pub fn drives() -> impl Iterator<Item = char> {
    std::iter::empty()
}

// ── helpers used by edit-wasm crate ──────────────────────────────────────────

/// Push VT-encoded bytes that the editor should treat as keyboard / mouse input
/// on the next [`read_stdin`] call.
pub fn wasm_push_input(s: &str) {
    STDIN_BUF.with(|buf| buf.borrow_mut().push_str(s));
}

/// Drain and return all VT output produced since the last call.
pub fn wasm_take_output() -> String {
    STDOUT_BUF.with(|buf| mem::take(&mut *buf.borrow_mut()))
}

/// Update the stored terminal size so that the next resize injection uses the
/// correct dimensions.
pub fn wasm_set_size(cols: u16, rows: u16) {
    TERMINAL_SIZE.with(|s| *s.borrow_mut() = (cols, rows));
}
