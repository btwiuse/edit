# Edit – Browser / WASM

This directory contains the browser front-end that lets you run the **Edit**
terminal editor entirely in your browser via WebAssembly and
[xterm.js](https://xtermjs.org/).

## How it works

```
keyboard/mouse
      │
      ▼
  xterm.js  ──(VT bytes)──▶  edit-wasm (Rust/WASM)  ──(VT bytes)──▶  xterm.js
                                     │
                              renders to in-memory
                              VT byte buffer
```

* The Rust editor already communicates entirely through VT/ANSI escape
  sequences on stdin/stdout.
* The `crates/edit-wasm` crate provides a WASM-compatible platform layer
  (`sys/wasm.rs`) that replaces OS calls with in-memory buffers.
* `wasm-bindgen` exposes three JavaScript functions: `editor_init`,
  `editor_step`, and `editor_resize`.
* `main.js` connects xterm.js events to those functions and writes the
  returned VT bytes back to xterm.js.

## Build & Run

### Prerequisites

```sh
# Install Rust (https://rustup.rs) then add the WASM target:
rustup target add wasm32-unknown-unknown

# Install wasm-pack:
cargo install wasm-pack

# Install a simple HTTP server (any will do):
npm install -g serve
# or: pip install --user httpserver
```

### Build the WASM package

Run from the repository root:

```sh
wasm-pack build crates/edit-wasm \
  --target web \
  --out-dir ../../web/pkg \
  -- --no-default-features
```

This compiles the editor to `web/pkg/edit_wasm.js` + `web/pkg/edit_wasm_bg.wasm`.

### Serve

```sh
serve web/
# Then open http://localhost:3000
```

Or with Python:

```sh
python3 -m http.server --directory web/ 8080
# Then open http://localhost:8080
```

## Keyboard shortcuts

All the usual editor shortcuts work:

| Shortcut      | Action                    |
|---------------|---------------------------|
| Ctrl+N        | New file                  |
| Ctrl+O        | Open file (file picker)   |
| Ctrl+S        | Save                      |
| Ctrl+Shift+S  | Save As                   |
| Ctrl+W        | Close tab                 |
| Ctrl+F        | Find                      |
| Ctrl+R        | Find & Replace            |
| Ctrl+G        | Go to line                |
| Ctrl+Q        | Quit                      |
| F10           | Focus menu bar            |

## Limitations

* **No real filesystem** – Save / Open use the browser's `<input type="file">`
  equivalent (the file picker shows only in-memory paths).  Future work could
  integrate the
  [File System Access API](https://developer.mozilla.org/en-US/docs/Web/API/File_System_Access_API).
* **No ICU** – Regular-expression search/replace is not available (the basic
  search is still functional).
* **Single-threaded** – WASM runs on the main thread.
* **8 MiB per document** – The virtual-memory gap buffer is capped at 8 MiB
  per open document to stay within browser memory budgets.
