# mdterm

A keyboard-driven Markdown and plain-text viewer/editor for Linux and macOS terminals. It uses direct native terminal calls, Comrak for Markdown parsing, and Unicode-aware text layout.

## Install

With Rust and Cargo installed, run this from the project directory:

```sh
cargo install --path . --locked
```

This installs `mdterm` to `~/.cargo/bin` by default. You can then launch it from any directory:

```sh
mdterm
mdterm README.md
mdterm /path/to/folder
```

If your shell cannot find `mdterm`, add Cargo's binary directory to your `PATH`:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

Add that line to your shell configuration (for example, `~/.bashrc` for Bash) to keep it across sessions. After updating the source, rerun the install command to upgrade.

## Run from source

```sh
cargo run
cargo run -- README.md
```

The file browser is a fixed left panel from startup, with the document on the right. With no path, choose a file from the current directory; passing a directory starts the browser there. Passing a file opens it in preview, and a path that does not exist starts a new document in edit mode.

The layout fills the terminal to its edges, without an app header or outer margins. It includes a continuous border with a shared divider between the browser and document, internal padding, no pane titles or path bars, a highlighted file selection, and a visible focus indicator. Unsaved edits appear as an asterisk beside the active filename. The bottom hints sit in a bordered footer joined to the panes and show the actions for the focused pane, with complete shortcuts wrapped across lines. Confirmation and input prompts replace those hints while active. It inherits your terminal's foreground and background colors, using bold, dim, and reverse video for emphasis. Preview text has a small, fixed amount of internal padding; the editor has line numbers and scrolls horizontally to keep the cursor visible. Tab switches panes and Ctrl+O toggles the browser. Below 60 columns, the browser uses the full screen; selecting a file returns to the document. Resizing the terminal updates the layout immediately.

## Keys

| Context | Keys |
| --- | --- |
| Preview | Up/Down or j/k to scroll, Page Up/Down to page, Ctrl+E to edit, Tab to focus the file browser, Ctrl+O to show or hide it, q to quit |
| Editor | Arrow keys to move, Home/End to move within a line, Enter to insert a line break, Backspace/Delete to remove text, Ctrl+S to save, Ctrl+Z / Ctrl+Y to undo/redo, Ctrl+E or Esc to preview, Tab to focus the file browser, Ctrl+O to show or hide it |
| File browser | Up/Down to select, Enter to open or enter a directory, Backspace to go up, n to create, r to rename, d to delete, Tab to focus the document, Ctrl+O to hide it, q or Ctrl+Q to quit |

File actions work on `.md`, `.markdown`, `.txt`, and `.text` files. New files default to `.md`; deletion asks for confirmation. Switching files, creating a document, deleting the active file, or quitting with unsaved edits offers save, discard, or Esc to cancel. When the browser is open in a terminal narrower than 60 columns, it takes over the screen; opening a file returns to the full-width document.

## Markdown preview

The preview renders CommonMark and GitHub-style Markdown, including:

- Six heading levels, nested emphasis, code, paragraphs, hard breaks, rules, escaping, and entities.
- Nested ordered/unordered lists, task checkboxes, blockquotes, and alerts.
- Tables with cell wrapping and alignment; narrow panes show labelled rows.
- Links, reference links, autolinks, wiki links, and footnotes. Link destinations are numbered and deduplicated at the end.
- Strikethrough, superscript/subscript, highlighting, inserted text, visible spoilers, emoji shortcodes, definition lists, and multiline quotes.
- YAML/TOML frontmatter shown verbatim; fenced and indented code preserve whitespace, expand tabs, and wrap with continuation markers.
- Embedded HTML text, formatting, lists, tables, and expanded details sections.

Images show alt text and their destination. Math shows TeX source; diagrams show their source. Scripts and styles are ignored, and rendering never executes content or fetches remote resources. Styling inherits terminal colors. Grapheme-aware wrapping handles combining characters, wide text, and emoji. Preview parsing is cached and refreshed after edits, undo, or redo; resizing reflows the document. Plain-text files keep their literal source.

Try the feature showcase:

```sh
cargo run -- tests/fixtures/markdown-showcase.md
```

## Regression checks

```sh
cargo test
cargo build
python3 tests/terminal_layout.py
```

The checks run the executable in a pseudo-terminal to exercise pane alignment, focus, resize behavior, scrolling, editing, and unsaved-change prompts.
