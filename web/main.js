// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

/**
 * Browser glue between xterm.js and the Edit WASM module.
 *
 * Build the WASM package before opening this page:
 *
 *   cargo install wasm-pack
 *   wasm-pack build crates/edit-wasm --target web --out-dir ../../web/pkg
 *
 * Then serve the web/ directory with any static HTTP server.
 */

import init, { editor_init, editor_step, editor_resize } from "./pkg/edit_wasm.js";

// ── helpers ───────────────────────────────────────────────────────────────────

/**
 * Write a string of VT bytes produced by the Rust editor to the xterm.js
 * terminal.  The terminal already understands VT/ANSI sequences, so we can
 * pass the bytes through verbatim.
 *
 * @param {import("xterm").Terminal} term
 * @param {string} vtOutput
 */
function writeOutput(term, vtOutput) {
  if (vtOutput && vtOutput.length > 0) {
    term.write(vtOutput);
  }
}

// ── main ──────────────────────────────────────────────────────────────────────

async function main() {
  const loadingEl = document.getElementById("loading");
  const containerEl = document.getElementById("terminal-container");

  // ── set up xterm.js ────────────────────────────────────────────────────────

  const term = new Terminal({
    // Use a colour scheme that matches the editor's default theme.
    theme: {
      background: "#1e1e1e",
      foreground: "#d4d4d4",
      cursor:     "#d4d4d4",
      // Standard 16 ANSI colours (VS Code Dark+ palette).
      black:         "#000000",
      red:           "#cd3131",
      green:         "#0dbc79",
      yellow:        "#e5e510",
      blue:          "#2472c8",
      magenta:       "#bc3fbc",
      cyan:          "#11a8cd",
      white:         "#e5e5e5",
      brightBlack:   "#666666",
      brightRed:     "#f14c4c",
      brightGreen:   "#23d18b",
      brightYellow:  "#f5f543",
      brightBlue:    "#3b8eea",
      brightMagenta: "#d670d6",
      brightCyan:    "#29b8db",
      brightWhite:   "#e5e5e5",
    },
    allowProposedApi: true,
    scrollback: 0,          // The editor manages its own scroll
    convertEol: false,
    cursorBlink: true,
  });

  const fitAddon = new FitAddon.FitAddon();
  term.loadAddon(fitAddon);
  term.open(containerEl);
  fitAddon.fit();

  // ── initialise WASM ────────────────────────────────────────────────────────

  await init();

  const { cols, rows } = term;
  const initialOutput = editor_init(cols, rows);
  writeOutput(term, initialOutput);

  // Hide the loading overlay.
  loadingEl.classList.add("hidden");
  term.focus();

  // ── wire up input ──────────────────────────────────────────────────────────

  // xterm.js gives us raw VT bytes for every key press / mouse event / paste.
  term.onData((data) => {
    const output = editor_step(data);
    writeOutput(term, output);
  });

  // Mouse reporting: xterm.js emits VT mouse sequences on its own when the
  // editor enables the mouse tracking modes (sent during editor_init).
  // We re-enable mouse support in xterm after the editor's setup sequence.
  term.onBinary((data) => {
    const output = editor_step(data);
    writeOutput(term, output);
  });

  // ── wire up resize ────────────────────────────────────────────────────────

  const resizeObserver = new ResizeObserver(() => {
    fitAddon.fit();
    const output = editor_resize(term.cols, term.rows);
    writeOutput(term, output);
  });
  resizeObserver.observe(containerEl);

  // Also handle explicit window resize events as a fallback.
  window.addEventListener("resize", () => {
    fitAddon.fit();
    const output = editor_resize(term.cols, term.rows);
    writeOutput(term, output);
  });
}

main().catch((err) => {
  console.error("Failed to start editor:", err);
  const loadingEl = document.getElementById("loading");
  if (loadingEl) {
    loadingEl.textContent = "Failed to load editor: " + err.message;
  }
});
