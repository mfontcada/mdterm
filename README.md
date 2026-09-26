# mdterm

A keyboard-driven Markdown and plain-text viewer/editor for Linux and macOS terminals. It uses direct native terminal calls, Comrak for Markdown parsing, and Unicode-aware text layout.

## Install

Install the latest release:

```sh
curl -fsSL https://github.com/mfontcada/mdterm/releases/latest/download/install.sh | sh
```

The installer downloads the executable for your platform, checks its SHA-256 checksum and version, and installs it into `~/.local/bin`. It requires a POSIX shell, `curl`, `tar`, and either `sha256sum` or `shasum`, normally provided by the operating system. Rust and sudo are not required. An existing installation is preserved if download or verification fails.

| System | Supported CPUs | Release executable |
| --- | --- | --- |
| Linux | x86-64, ARM64 | Statically linked with musl; no glibc version dependency |
| macOS 11 or later | Intel x86-64, Apple Silicon | Native executable for each CPU |

The macOS binaries target macOS 11 and are tested on the GitHub-hosted macOS runners. They are not Apple Developer ID signed or notarized.

If `~/.local/bin` is not on your `PATH`, run:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Add that line to your shell configuration (`~/.bashrc` for Bash or `~/.zshrc` for Zsh) to keep it across sessions. Putting this directory first also selects the new binary if you previously installed another copy with Cargo. Check with `command -v mdterm`.

Launch from any directory:

```sh
mdterm
mdterm README.md
mdterm /path/to/folder
mdterm --version
```

### Update or choose a version

Rerun the installation command to replace the executable with the latest stable release, then run `mdterm --version`. Updates run only when you request them.

To install a specific release:

```sh
curl -fsSL https://github.com/mfontcada/mdterm/releases/latest/download/install.sh | MDTERM_VERSION=v0.1.0 sh
```

To use a different installation directory:

```sh
curl -fsSL https://github.com/mfontcada/mdterm/releases/latest/download/install.sh | MDTERM_INSTALL_DIR="$HOME/bin" sh
```

### Manual download

Download the archive for your platform and `SHA256SUMS` from [GitHub Releases](https://github.com/mfontcada/mdterm/releases/latest). The filenames use Rust target names: `x86_64` for Intel/AMD, `aarch64` for ARM64, `unknown-linux-musl` for Linux, and `apple-darwin` for macOS.

Compute the archive's checksum with `sha256sum <archive>` on Linux or `shasum -a 256 <archive>` on macOS, and compare it with the matching entry in `SHA256SUMS`. For example, after verifying the Linux x86-64 archive:

```sh
tar -xzf mdterm-v0.1.0-x86_64-unknown-linux-musl.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 755 mdterm "$HOME/.local/bin/mdterm"
```

Each archive contains the executable, this README, and the MIT license.

### Uninstall

Remove the executable from its installation directory:

```sh
rm "$HOME/.local/bin/mdterm"
```

Your documents are unaffected. If you installed with Cargo, use `cargo uninstall mdterm` instead.

### Install with Cargo

With Rust, Cargo, and your platform's build tools installed, you can build directly from GitHub without a manual clone:

```sh
cargo install --git https://github.com/mfontcada/mdterm --locked
```

Or, from a local checkout:

```sh
cargo install --path . --locked
```

Cargo installs into `~/.cargo/bin` by default; ensure that directory is on your `PATH`. Rerun the corresponding Cargo command to update that installation.

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
python3 -m unittest discover -s tests -p 'test_*.py' -v
```

The checks run the executable in a pseudo-terminal to exercise pane alignment, focus, resize behavior, scrolling, editing, and unsaved-change prompts.

Set `MDTERM_BINARY` to an absolute executable path to run terminal and installer checks against a release build. The installer checks use temporary directories and simulated downloads; they do not modify your home directory or contact GitHub. Release helper checks require Python 3.11 or later.

## Publishing a release

1. Update the package version in `Cargo.toml` and run `cargo check` to refresh the root package entry in `Cargo.lock`.
2. Run the regression checks, commit the changes, and push `main`.
3. Optionally run `gh workflow run release.yml --ref main` to validate all four platforms without publishing.
4. Tag that commit with the matching stable version (`vMAJOR.MINOR.PATCH`) and push the tag. For example, after updating the package to `0.2.0`:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

The release workflow builds and tests all four executables, then uploads their archives directly into a draft release. Once every build passes, it adds `SHA256SUMS` and `install.sh` and publishes the release. It then installs the public downloads and runs terminal checks on every platform. Follow progress in [Actions](https://github.com/mfontcada/mdterm/actions/workflows/release.yml).

Failed builds leave the release unpublished. Rerunning a failed tagged workflow can resume the draft; published releases are never overwritten. Use a new version for changes to an already published release.

The workflow runs only in public repositories, uses standard GitHub-hosted runners, and stores no Actions artifacts or dependency caches. Release files are hosted as GitHub Release assets. To block paid Actions usage, maintainers should keep an Actions budget of `$0` with **Stop usage when budget limit is reached** enabled in GitHub's billing settings.
