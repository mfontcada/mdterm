# mdterm

A keyboard-driven Markdown and plain-text viewer/editor for your terminal.

## Install

Available for Linux (x86-64, ARM64) and macOS 11+ (Intel, Apple Silicon).

```sh
curl -fsSL https://github.com/mfontcada/mdterm/releases/latest/download/install.sh | sh
```

Installs to `~/.local/bin`. Rust and sudo are not required.

If `mdterm` is not found after installation, add its directory to your PATH:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Add that line to `~/.bashrc` (Bash) or `~/.zshrc` (Zsh) to keep it across sessions.

## Update

Rerun the installation command to update to the latest release.

## Usage

```sh
mdterm
mdterm notes.md
mdterm /path/to/folder
mdterm --version
```

With no argument, browse files in the current directory. Open a file to preview it, then press Ctrl+E to edit.

## Shortcuts

| Action | Keys |
| --- | --- |
| Scroll preview | Up/Down or j/k; Page Up/Down |
| Toggle preview/edit | Ctrl+E; Esc returns to preview |
| Save | Ctrl+S |
| Undo / redo | Ctrl+Z / Ctrl+Y |
| Switch between document and files | Tab |
| Show / hide file browser | Ctrl+O |
| Open selected file / parent folder | Enter / Backspace in the file browser |
| Create / rename / delete file | n / r / d in the file browser |
| Quit | Ctrl+Q |

Unsaved changes prompt you to save, discard, or cancel.

## Uninstall

```sh
rm "$HOME/.local/bin/mdterm"
```

Your documents are unaffected.

## License

[MIT](LICENSE)
