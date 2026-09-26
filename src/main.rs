#![allow(clippy::missing_safety_doc)]

mod markdown;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

#[cfg(target_os = "linux")]
mod os {
    use std::io;
    use std::os::raw::{c_int, c_ulong};

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub iflag: u32,
        pub oflag: u32,
        pub cflag: u32,
        pub lflag: u32,
        pub line: u8,
        pub cc: [u8; 32],
        pub ispeed: u32,
        pub ospeed: u32,
    }

    #[repr(C)]
    pub struct WinSize {
        pub rows: u16,
        pub cols: u16,
        pub xpixel: u16,
        pub ypixel: u16,
    }

    unsafe extern "C" {
        fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
        fn tcsetattr(fd: c_int, actions: c_int, termios: *const Termios) -> c_int;
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    pub const VTIME: usize = 5;
    pub const VMIN: usize = 6;
    const ECHO: u32 = 0x0008;
    const ICANON: u32 = 0x0002;
    const ISIG: u32 = 0x0001;
    const IEXTEN: u32 = 0x8000;
    const ICRNL: u32 = 0x0100;
    const IXON: u32 = 0x0400;

    pub fn get() -> io::Result<Termios> {
        let mut value = unsafe { std::mem::zeroed() };
        if unsafe { tcgetattr(0, &mut value) } == -1 { Err(io::Error::last_os_error()) } else { Ok(value) }
    }

    pub fn set(value: &Termios) -> io::Result<()> {
        if unsafe { tcsetattr(0, 0, value) } == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    pub fn raw(mut value: Termios) -> Termios {
        value.lflag &= !(ECHO | ICANON | ISIG | IEXTEN);
        value.iflag &= !(ICRNL | IXON);
        value.cc[VTIME] = 1;
        value.cc[VMIN] = 0;
        value
    }

    pub fn size() -> (usize, usize) {
        let mut ws = WinSize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 };
        if unsafe { ioctl(1, 0x5413, &mut ws) } == 0 && ws.rows > 0 && ws.cols > 0 {
            (ws.rows as usize, ws.cols as usize)
        } else { (24, 80) }
    }
}

#[cfg(target_os = "macos")]
mod os {
    use std::io;
    use std::os::raw::{c_int, c_ulong};

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub iflag: u64,
        pub oflag: u64,
        pub cflag: u64,
        pub lflag: u64,
        pub cc: [u8; 20],
        pub ispeed: u64,
        pub ospeed: u64,
    }

    #[repr(C)]
    pub struct WinSize { pub rows: u16, pub cols: u16, pub xpixel: u16, pub ypixel: u16 }

    unsafe extern "C" {
        fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
        fn tcsetattr(fd: c_int, actions: c_int, termios: *const Termios) -> c_int;
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }

    pub const VTIME: usize = 17;
    pub const VMIN: usize = 16;
    const ECHO: u64 = 0x00000008;
    const ICANON: u64 = 0x00000100;
    const ISIG: u64 = 0x00000080;
    const IEXTEN: u64 = 0x00000400;
    const ICRNL: u64 = 0x00000100;
    const IXON: u64 = 0x00000200;

    pub fn get() -> io::Result<Termios> {
        let mut value = unsafe { std::mem::zeroed() };
        if unsafe { tcgetattr(0, &mut value) } == -1 { Err(io::Error::last_os_error()) } else { Ok(value) }
    }
    pub fn set(value: &Termios) -> io::Result<()> {
        if unsafe { tcsetattr(0, 0, value) } == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
    pub fn raw(mut value: Termios) -> Termios {
        value.lflag &= !(ECHO | ICANON | ISIG | IEXTEN);
        value.iflag &= !(ICRNL | IXON);
        value.cc[VTIME] = 1;
        value.cc[VMIN] = 0;
        value
    }
    pub fn size() -> (usize, usize) {
        let mut ws = WinSize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 };
        if unsafe { ioctl(1, 0x40087468, &mut ws) } == 0 && ws.rows > 0 && ws.cols > 0 {
            (ws.rows as usize, ws.cols as usize)
        } else { (24, 80) }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
compile_error!("mdterm currently supports Linux and macOS only");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key { Char(char), Ctrl(char), Up, Down, Left, Right, Home, End, PageUp, PageDown, Delete, Backspace, Enter, Esc, Unknown }

struct Terminal { original: os::Termios }
impl Terminal {
    fn enter() -> io::Result<Self> {
        let original = os::get()?;
        os::set(&os::raw(original))?;
        print!("\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
        io::stdout().flush()?;
        Ok(Self { original })
    }
    fn read_key(&self) -> io::Result<Key> {
        let mut stdin = io::stdin().lock();
        let mut b = [0u8; 1];
        if stdin.read(&mut b)? == 0 { return Ok(Key::Unknown); }
        let first = b[0];
        if first == 3 { return Ok(Key::Ctrl('c')); }
        if first == 13 || first == 10 { return Ok(Key::Enter); }
        if first == 127 || first == 8 { return Ok(Key::Backspace); }
        if first == 9 { return Ok(Key::Char('\t')); }
        if first == 27 {
            if stdin.read(&mut b)? == 0 { return Ok(Key::Esc); }
            if b[0] != b'[' { return Ok(Key::Esc); }
            if stdin.read(&mut b)? == 0 { return Ok(Key::Esc); }
            return Ok(match b[0] {
                b'A' => Key::Up, b'B' => Key::Down, b'C' => Key::Right, b'D' => Key::Left,
                b'H' => Key::Home, b'F' => Key::End,
                b'5' | b'6' => { let page = b[0]; let _ = stdin.read(&mut b)?; if page == b'5' { Key::PageUp } else { Key::PageDown } },
                b'3' => { let _ = stdin.read(&mut b)?; Key::Delete }, _ => Key::Unknown,
            });
        }
        if first < 32 { return Ok(Key::Ctrl((first + 96) as char)); }
        let mut bytes = vec![first];
        let width = if first & 0xE0 == 0xC0 { 2 } else if first & 0xF0 == 0xE0 { 3 } else if first & 0xF8 == 0xF0 { 4 } else { 1 };
        for _ in 1..width { if stdin.read(&mut b)? != 0 { bytes.push(b[0]); } }
        Ok(std::str::from_utf8(&bytes).ok().and_then(|s| s.chars().next()).map(Key::Char).unwrap_or(Key::Unknown))
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = os::set(&self.original);
        print!("\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

#[derive(Clone)]
struct Snapshot { lines: Vec<String>, row: usize, col: usize }
struct Buffer { lines: Vec<String>, row: usize, col: usize, undo: Vec<Snapshot>, redo: Vec<Snapshot>, dirty: bool, revision: u64 }
impl Buffer {
    fn new(text: &str) -> Self {
        let mut lines: Vec<String> = text.split('\n').map(|s| s.trim_end_matches('\r').to_owned()).collect();
        if lines.is_empty() { lines.push(String::new()); }
        Self { lines, row: 0, col: 0, undo: Vec::new(), redo: Vec::new(), dirty: false, revision: 0 }
    }
    fn snapshot(&self) -> Snapshot { Snapshot { lines: self.lines.clone(), row: self.row, col: self.col } }
    fn record(&mut self) { self.revision = self.revision.wrapping_add(1); self.undo.push(self.snapshot()); if self.undo.len() > 128 { self.undo.remove(0); } self.redo.clear(); }
    fn restore(&mut self, s: Snapshot) { self.revision = self.revision.wrapping_add(1); self.lines = s.lines; self.row = s.row; self.col = s.col; self.dirty = true; }
    fn text(&self) -> String { self.lines.join("\n") }
    fn clamp(&mut self) { self.row = self.row.min(self.lines.len().saturating_sub(1)); self.col = self.col.min(self.lines[self.row].chars().count()); }
    fn insert(&mut self, c: char) {
        self.record(); let ix = self.lines[self.row].char_indices().nth(self.col).map(|(i,_)| i).unwrap_or(self.lines[self.row].len());
        self.lines[self.row].insert(ix, c); self.col += 1; self.dirty = true;
    }
    fn newline(&mut self) {
        self.record(); let ix = self.lines[self.row].char_indices().nth(self.col).map(|(i,_)| i).unwrap_or(self.lines[self.row].len());
        let tail = self.lines[self.row].split_off(ix); self.row += 1; self.lines.insert(self.row, tail); self.col = 0; self.dirty = true;
    }
    fn backspace(&mut self) {
        if self.col > 0 { self.record(); let ix = self.lines[self.row].char_indices().nth(self.col-1).map(|(i,_)| i).unwrap_or(0); self.lines[self.row].remove(ix); self.col -= 1; self.dirty = true; }
        else if self.row > 0 { self.record(); let current = self.lines.remove(self.row); self.row -= 1; self.col = self.lines[self.row].chars().count(); self.lines[self.row].push_str(&current); self.dirty = true; }
    }
    fn delete(&mut self) {
        if self.col < self.lines[self.row].chars().count() { self.record(); let ix = self.lines[self.row].char_indices().nth(self.col).map(|(i,_)| i).unwrap_or(0); self.lines[self.row].remove(ix); self.dirty = true; }
        else if self.row + 1 < self.lines.len() { self.record(); let next = self.lines.remove(self.row+1); self.lines[self.row].push_str(&next); self.dirty = true; }
    }
    fn undo(&mut self) { if let Some(s) = self.undo.pop() { self.redo.push(self.snapshot()); self.restore(s); } }
    fn redo(&mut self) { if let Some(s) = self.redo.pop() { self.undo.push(self.snapshot()); self.restore(s); } }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Preview, Edit }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus { Browser, Document }
enum Prompt { Unsaved(AfterPrompt), Input(InputAction, String), Delete(PathBuf), Message(String) }
enum AfterPrompt { Exit, Open(PathBuf), Create(PathBuf), Delete(PathBuf) }
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputAction { Create, Rename }

struct App {
    mode: Mode, focus: Focus, browser_open: bool, file: Option<PathBuf>, buffer: Buffer,
    preview_cache: markdown::PreviewCache,
    picker: PathBuf, entries: Vec<PathBuf>, selected: usize, top: usize, browser_top: usize, prompt: Option<Prompt>, running: bool,
}
impl App {
    fn new(path: Option<PathBuf>) -> io::Result<Self> {
        let cwd = env::current_dir()?;
        let path = path.map(|p| if p.is_absolute() { p } else { cwd.join(p) });
        let mut app = Self { mode: Mode::Preview, focus: Focus::Browser, browser_open: true, file: None, buffer: Buffer::new(""), preview_cache: markdown::PreviewCache::default(), picker: cwd.clone(), entries: Vec::new(), selected: 0, top: 0, browser_top: 0, prompt: None, running: true };
        if let Some(path) = path {
            if path.is_dir() { app.picker = fs::canonicalize(path)?; app.refresh_picker()?; }
            else {
                app.open_file(path)?;
                if !app.file.as_ref().map(|p| p.exists()).unwrap_or(false) { app.mode = Mode::Edit; }
            }
        } else { app.refresh_picker()?; }
        Ok(app)
    }
    fn open_file(&mut self, path: PathBuf) -> io::Result<()> {
        let path = if path.exists() { fs::canonicalize(path)? } else { path };
        let text = if path.exists() { fs::read_to_string(&path)? } else { String::new() };
        if let Some(parent) = path.parent() { self.picker = if parent.as_os_str().is_empty() { PathBuf::from(".") } else { parent.to_path_buf() }; }
        self.file = Some(path);
        self.buffer = Buffer::new(&text);
        self.preview_cache = markdown::PreviewCache::default();
        self.mode = Mode::Preview;
        self.focus = Focus::Document;
        self.browser_open = os::size().1 >= 60;
        self.top = 0;
        if self.picker.is_dir() {
            if self.refresh_picker().is_err() {
                self.entries.clear();
                self.selected = 0;
            }
        } else {
            self.entries.clear();
            self.selected = 0;
        }
        Ok(())
    }
    fn refresh_picker(&mut self) -> io::Result<()> {
        self.entries.clear();
        if let Some(parent) = self.picker.parent() { self.entries.push(parent.to_path_buf()); }
        let mut dirs = Vec::new(); let mut files = Vec::new();
        for entry in fs::read_dir(&self.picker)? {
            let entry = entry?; let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') { continue; }
            let path = entry.path();
            if entry.file_type()?.is_dir() { dirs.push(path); }
            else if entry.file_type()?.is_file() && supported_text(&path) { files.push(path); }
        }
        dirs.sort(); files.sort(); self.entries.extend(dirs); self.entries.extend(files);
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        if let Some(current) = self.file.as_ref() {
            let parent = current.parent().unwrap_or_else(|| Path::new("."));
            let same_dir = parent == self.picker.as_path() || (parent.as_os_str().is_empty() && self.picker == PathBuf::from("."));
            if same_dir {
                if let Some(name) = current.file_name() {
                    if let Some(index) = self.entries.iter().position(|p| p.file_name() == Some(name)) { self.selected = index; }
                }
            }
        }
        self.browser_top = 0; Ok(())
    }
    fn save(&mut self) -> io::Result<()> {
        let path = self.file.as_ref().ok_or_else(|| io::Error::new(io::ErrorKind::Other, "no file selected"))?;
        fs::write(path, self.buffer.text())?;
        self.buffer.dirty = false;
        if self.picker.is_dir() { self.refresh_picker()?; }
        Ok(())
    }
    fn handle(&mut self, key: Key) -> io::Result<()> {
        if self.prompt.is_some() { return self.prompt_key(key); }
        if key == Key::Ctrl('c') { self.leave_app(); return Ok(()); }
        if key == Key::Ctrl('o') {
            self.browser_open = !self.browser_open;
            self.focus = if self.browser_open { Focus::Browser } else { Focus::Document };
            return Ok(());
        }
        if key == Key::Char('\t') {
            if self.browser_open && os::size().1 >= 60 {
                self.focus = if self.focus == Focus::Browser { Focus::Document } else { Focus::Browser };
            }
            return Ok(());
        }
        let narrow_browser = self.browser_open && os::size().1 < 60;
        if narrow_browser || (self.browser_open && self.focus == Focus::Browser) {
            self.picker_key(key)
        } else {
            match self.mode { Mode::Preview => self.preview_key(key), Mode::Edit => self.edit_key(key) }
        }
    }
    fn preview_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Ctrl('e') if self.file.is_some() => self.mode = Mode::Edit,
            Key::Char('q') | Key::Ctrl('q') => self.leave_app(),
            Key::Down | Key::Char('j') => self.top += 1,
            Key::Up | Key::Char('k') => self.top = self.top.saturating_sub(1),
            Key::PageDown => self.top += 10,
            Key::PageUp => self.top = self.top.saturating_sub(10),
            _ => {}
        }
        Ok(())
    }
    fn edit_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Ctrl('e') | Key::Esc => self.mode = Mode::Preview,
            Key::Ctrl('s') => if let Err(e) = self.save() { self.prompt = Some(Prompt::Message(format!("Save failed: {e}"))); },
            Key::Ctrl('q') => self.leave_app(),
            Key::Ctrl('z') => self.buffer.undo(), Key::Ctrl('y') => self.buffer.redo(),
            Key::Up => { self.buffer.row = self.buffer.row.saturating_sub(1); self.buffer.clamp(); },
            Key::Down => { self.buffer.row = (self.buffer.row + 1).min(self.buffer.lines.len()-1); self.buffer.clamp(); },
            Key::Left => if self.buffer.col > 0 { self.buffer.col -= 1; } else if self.buffer.row > 0 { self.buffer.row -= 1; self.buffer.col = self.buffer.lines[self.buffer.row].chars().count(); },
            Key::Right => if self.buffer.col < self.buffer.lines[self.buffer.row].chars().count() { self.buffer.col += 1; } else if self.buffer.row + 1 < self.buffer.lines.len() { self.buffer.row += 1; self.buffer.col = 0; },
            Key::Home => self.buffer.col = 0,
            Key::End => self.buffer.col = self.buffer.lines[self.buffer.row].chars().count(),
            Key::Backspace => self.buffer.backspace(), Key::Delete => self.buffer.delete(), Key::Enter => self.buffer.newline(),
            Key::Char(c) if !c.is_control() => self.buffer.insert(c),
            _ => {}
        }
        Ok(())
    }
    fn picker_key(&mut self, key: Key) -> io::Result<()> {
        match key {
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Down => self.selected = (self.selected + 1).min(self.entries.len().saturating_sub(1)),
            Key::Backspace => if let Some(parent) = self.picker.parent() { self.picker = parent.to_path_buf(); self.refresh_picker()?; },
            Key::Enter => if let Some(path) = self.entries.get(self.selected).cloned() {
                if path.is_dir() { self.picker = path; self.refresh_picker()?; }
                else if self.file.as_ref() == Some(&path) {
                    if os::size().1 < 60 { self.browser_open = false; }
                    self.focus = Focus::Document;
                }
                else if self.buffer.dirty { self.prompt = Some(Prompt::Unsaved(AfterPrompt::Open(path))); }
                else { self.open_file(path)?; }
            },
            Key::Char('n') => self.prompt = Some(Prompt::Input(InputAction::Create, String::new())),
            Key::Char('r') => if let Some(path) = self.entries.get(self.selected).cloned().filter(|p| p.is_file()) {
                self.prompt = Some(Prompt::Input(InputAction::Rename, path.file_name().unwrap_or_default().to_string_lossy().into_owned()));
            },
            Key::Char('d') => if let Some(path) = self.entries.get(self.selected).cloned().filter(|p| p.is_file()) { self.prompt = Some(Prompt::Delete(path)); },
            Key::Char('q') | Key::Ctrl('q') => self.leave_app(),
            _ => {}
        }
        Ok(())
    }
    fn leave_app(&mut self) {
        if self.buffer.dirty { self.prompt = Some(Prompt::Unsaved(AfterPrompt::Exit)); } else { self.running = false; }
    }
    fn prompt_key(&mut self, key: Key) -> io::Result<()> {
        let prompt = self.prompt.take().unwrap();
        match prompt {
            Prompt::Message(_) => {},
            Prompt::Unsaved(after) => match key {
                Key::Char('y') | Key::Char('Y') => {
                    if let Err(e) = self.save() { self.prompt = Some(Prompt::Message(format!("Save failed: {e}"))); return Ok(()); }
                    self.after_prompt(after)?;
                },
                Key::Char('n') | Key::Char('N') => self.after_prompt(after)?,
                Key::Esc => {},
                _ => self.prompt = Some(Prompt::Unsaved(after)),
            },
            Prompt::Delete(path) => match key {
                Key::Char('y') | Key::Char('Y') => {
                    if self.file.as_ref() == Some(&path) && self.buffer.dirty { self.prompt = Some(Prompt::Unsaved(AfterPrompt::Delete(path))); }
                    else { self.delete_file(path)?; }
                },
                Key::Char('n') | Key::Char('N') | Key::Esc => {}, _ => self.prompt = Some(Prompt::Delete(path)),
            },
            Prompt::Input(action, mut value) => match key {
                Key::Enter => self.finish_input(action, value)?,
                Key::Esc => {},
                Key::Backspace => { value.pop(); self.prompt = Some(Prompt::Input(action, value)); },
                Key::Char(c) if !c.is_control() && value.len() < 240 => { value.push(c); self.prompt = Some(Prompt::Input(action, value)); },
                _ => self.prompt = Some(Prompt::Input(action, value)),
            },
        }
        Ok(())
    }
    fn after_prompt(&mut self, after: AfterPrompt) -> io::Result<()> {
        match after {
            AfterPrompt::Exit => self.running = false,
            AfterPrompt::Open(path) => self.open_file(path)?,
            AfterPrompt::Create(path) => {
                fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
                self.open_file(path)?;
                self.mode = Mode::Edit;
            },
            AfterPrompt::Delete(path) => self.delete_file(path)?,
        }
        Ok(())
    }
    fn delete_file(&mut self, path: PathBuf) -> io::Result<()> {
        if let Err(e) = fs::remove_file(&path) {
            self.prompt = Some(Prompt::Message(format!("Delete failed: {e}")));
            return Ok(());
        }
        if self.file.as_ref() == Some(&path) {
            self.file = None;
            self.buffer = Buffer::new("");
            self.preview_cache = markdown::PreviewCache::default();
            self.mode = Mode::Preview;
            self.focus = Focus::Browser;
            self.browser_open = true;
        }
        self.refresh_picker()
    }
    fn finish_input(&mut self, action: InputAction, name: String) -> io::Result<()> {
        let name = name.trim(); if name.is_empty() { return Ok(()); }
        let mut target = self.picker.join(name);
        match action {
            InputAction::Create => {
                if target.extension().is_none() { target.set_extension("md"); }
                if !supported_text(&target) { self.prompt = Some(Prompt::Message("Use a .md, .markdown, .txt, or .text filename".into())); return Ok(()); }
                if target.exists() { self.prompt = Some(Prompt::Message("File already exists".into())); return Ok(()); }
                if self.buffer.dirty { self.prompt = Some(Prompt::Unsaved(AfterPrompt::Create(target))); }
                else { self.after_prompt(AfterPrompt::Create(target))?; }
            }
            InputAction::Rename => {
                let Some(source) = self.entries.get(self.selected).cloned() else { return Ok(()); };
                if target.extension().is_none() { if let Some(ext) = source.extension() { target.set_extension(ext); } }
                if target == source { return Ok(()); }
                if !supported_text(&target) { self.prompt = Some(Prompt::Message("Use a .md, .markdown, .txt, or .text filename".into())); return Ok(()); }
                if target.exists() { self.prompt = Some(Prompt::Message("File already exists".into())); return Ok(()); }
                fs::rename(&source, &target)?;
                if self.file.as_ref() == Some(&source) { self.file = Some(target); }
                self.refresh_picker()?;
            }
        }
        Ok(())
    }
    fn draw(&mut self) -> io::Result<()> {
        let (rows, cols) = os::size();
        let frame = self.render(rows, cols);
        print!("{frame}");
        io::stdout().flush()
    }

    fn hints(&self, browser_focused: bool, split: bool, width: usize) -> Vec<String> {
        let actions: Vec<String> = match &self.prompt {
            Some(Prompt::Unsaved(_)) => vec!["Save changes?".into(), "y save".into(), "n discard".into(), "Esc cancel".into()],
            Some(Prompt::Input(action, value)) => vec![format!("{} name: {}", if *action == InputAction::Create { "New file" } else { "Rename" }, value), "Enter confirm".into(), "Esc cancel".into()],
            Some(Prompt::Delete(path)) => vec![format!("Delete {}?", path.file_name().unwrap_or_default().to_string_lossy()), "y delete".into(), "n / Esc cancel".into()],
            Some(Prompt::Message(message)) => vec![message.clone(), "Any key dismiss".into()],
            None => {
                let mut actions: Vec<&str> = if browser_focused {
                    vec!["Enter open", "↑/↓ select", "Backspace parent", "n new", "r rename", "d delete"]
                } else if self.mode == Mode::Edit && self.file.is_some() {
                    vec!["Ctrl+S save", "Esc preview", "Arrows move", "Ctrl+Z undo", "Ctrl+Y redo"]
                } else if self.file.is_some() {
                    vec!["↑/↓ scroll", "PgUp/PgDn page", "Ctrl+E edit"]
                } else { vec![] };
                if split { actions.push(if browser_focused { "Tab focus document" } else { "Tab focus files" }); }
                actions.push(if self.browser_open { "Ctrl+O hide files" } else { "Ctrl+O show files" });
                actions.push("Ctrl+Q quit");
                actions.into_iter().map(str::to_owned).collect()
            }
        };
        let mut lines = Vec::new();
        let mut line = String::new();
        for action in actions {
            let cells: usize = action.chars().map(cell_width).sum();
            if !line.is_empty() && line.chars().map(cell_width).sum::<usize>() + cells + 3 > width {
                lines.push(line); line = String::new();
            }
            if !line.is_empty() { line.push_str("   "); }
            line.push_str(&action);
        }
        if !line.is_empty() { lines.push(line); }
        lines
    }

    fn render(&mut self, rows: usize, cols: usize) -> String {
        let mut out = String::from("\x1b[?25l");
        if rows < 7 || cols < 24 {
            for row in 0..rows { paint(&mut out, row, 0, cols, "", SURFACE); }
            paint(&mut out, 0, 0, cols, " Enlarge terminal", MUTED);
            return out;
        }
        let browser_only = self.browser_open && cols < 60;
        let split = self.browser_open && !browser_only;
        let side = if split { (cols / 3).clamp(20, 30) } else { 0 };
        let document_x = if split { side } else { 0 };
        let document_width = cols - document_x;
        let browser_focused = self.browser_open && (browser_only || self.focus == Focus::Browser);
        let document_focused = !browser_only && self.focus == Focus::Document;
        let hints = self.hints(browser_focused, split, cols - 4);
        let footer_height = hints.len().max(1);
        if rows < footer_height + 5 {
            for row in 0..rows { paint(&mut out, row, 0, cols, "", SURFACE); }
            paint(&mut out, 0, 0, cols, " Enlarge terminal", MUTED);
            return out;
        }
        let bottom = rows - footer_height - 2;
        let content_height = bottom - 1;

        for row in 0..rows { paint(&mut out, row, 0, cols, "", SURFACE); }

        if self.browser_open {
            let width = if browser_only { cols } else { side + 1 };
            pane_border(&mut out, 0, width, bottom, browser_focused);
            keep_visible(&mut self.browser_top, self.selected, content_height);
            for i in 0..content_height {
                let index = self.browser_top + i;
                if let Some(path) = self.entries.get(index) {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let label = if self.picker.parent() == Some(path.as_path()) { "../".to_owned() }
                        else if path.is_dir() { format!("{name}/") } else { name.into_owned() };
                    let selected = index == self.selected;
                    let current = self.file.as_ref() == Some(path);
                    let marker = if selected { ">" } else if current { "*" } else { " " };
                    let style = if selected && browser_focused { SELECTED } else if selected { INACTIVE_SELECTED } else { SIDEBAR };
                    let dirty = if current && self.buffer.dirty { " *" } else { "" };
                    paint(&mut out, 1 + i, 2, width - 4, &format!("{marker} {label}{dirty}"), style);
                }
            }
        }
        if !browser_only {
            pane_border(&mut out, document_x, document_width, bottom, document_focused);
            if self.file.is_none() {
                let x = document_x + 2;
                let w = document_width.saturating_sub(4);
                for (i, line) in ["Open a document", "", "Select a file and press Enter."].iter().enumerate() {
                    if i < content_height { paint(&mut out, 1 + i, x, w, line, if i == 0 { EMPHASIS } else { MUTED }); }
                }
            } else if self.mode == Mode::Edit {
                let gutter = self.buffer.lines.len().to_string().len() + 2;
                let text_width = document_width.saturating_sub(gutter + 3).max(1);
                keep_visible(&mut self.top, self.buffer.row, content_height);
                let cursor_cells: usize = column_width(&self.buffer.lines[self.buffer.row], self.buffer.col);
                let horizontal = cursor_cells.saturating_sub(text_width - 1);
                for i in 0..content_height {
                    let index = self.top + i;
                    if let Some(line) = self.buffer.lines.get(index) {
                        paint(&mut out, 1 + i, document_x + 1, gutter, &format!(" {:>width$} ", index + 1, width = gutter - 2), MUTED);
                        paint(&mut out, 1 + i, document_x + 1 + gutter, text_width, &skip_cells(line, horizontal), if index == self.buffer.row { CURRENT_LINE } else { SURFACE });
                    }
                }
            } else {
                let text_width = document_width.saturating_sub(4).max(1);
                let inset = 2;
                let is_markdown = self.file.as_ref().and_then(|p| p.extension()).and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
                let document = self.preview_cache.lines(&self.buffer.lines, self.buffer.revision, is_markdown, text_width);
                self.top = self.top.min(document.len().saturating_sub(content_height));
                for i in 0..content_height {
                    if let Some(line) = document.get(self.top + i) {
                        out.push_str(&format!("\x1b[{};{}H{}", 2 + i, document_x + inset + 1, line.ansi(text_width)));
                    }
                }
            }
        }
        if split {
            paint(&mut out, 0, side, 1, "┬", BORDER);
            for row in 1..bottom { paint(&mut out, row, side, 1, "│", BORDER); }
            paint(&mut out, bottom, side, 1, "┴", BORDER);
        }
        // The shortcuts share their top border with the panes, with no outer margin.
        paint(&mut out, bottom, 0, 1, "├", BORDER);
        paint(&mut out, bottom, cols - 1, 1, "┤", BORDER);
        for row in bottom + 1..rows - 1 {
            paint(&mut out, row, 0, 1, "│", BORDER);
            paint(&mut out, row, cols - 1, 1, "│", BORDER);
        }
        for (i, hint) in hints.iter().enumerate() {
            paint(&mut out, bottom + 1 + i, 2, cols - 4, hint, if self.prompt.is_some() { PROMPT_STYLE } else { FOOTER });
        }
        paint(&mut out, rows - 1, 0, cols, &format!("└{}┘", "─".repeat(cols - 2)), BORDER);
        // Painting moves the terminal cursor; restore it only after the complete frame.
        if self.mode == Mode::Edit && document_focused && self.file.is_some() && self.prompt.is_none() {
            let gutter = self.buffer.lines.len().to_string().len() + 2;
            let text_width = document_width.saturating_sub(gutter + 3).max(1);
            let cursor_cells: usize = column_width(&self.buffer.lines[self.buffer.row], self.buffer.col);
            out.push_str(&format!("\x1b[{};{}H\x1b[?25h", 2 + self.buffer.row - self.top, document_x + gutter + cursor_cells.min(text_width - 1) + 2));
        }
        out.push_str("\x1b[0m");
        out
    }

}

fn supported_text(path: &Path) -> bool {
    matches!(path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase().as_str(), "md" | "markdown" | "txt" | "text")
}
// Inherit the terminal's foreground and background; distinguish UI roles with attributes.
const SURFACE: &str = "\x1b[0m";
const SIDEBAR: &str = SURFACE;
const MUTED: &str = "\x1b[0;2m";
const ACTIVE: &str = EMPHASIS;
const SELECTED: &str = "\x1b[0;7m";
const INACTIVE_SELECTED: &str = EMPHASIS;
const BORDER: &str = MUTED;
const EMPHASIS: &str = "\x1b[0;1m";
const CURRENT_LINE: &str = SURFACE;
const FOOTER: &str = SURFACE;
const PROMPT_STYLE: &str = "\x1b[0;1;7m";

fn pane_border(out: &mut String, x: usize, width: usize, bottom: usize, active: bool) {
    let style = if active { ACTIVE } else { BORDER };
    paint(out, 0, x, width, &format!("┌{}┐", "─".repeat(width - 2)), style);
    paint(out, bottom, x, width, &format!("└{}┘", "─".repeat(width - 2)), style);
    for row in 1..bottom {
        paint(out, row, x, 1, "│", style);
        paint(out, row, x + width - 1, 1, "│", style);
    }

}

fn keep_visible(top: &mut usize, selected: usize, height: usize) {
    if selected < *top { *top = selected; }
    if selected >= top.saturating_add(height) { *top = selected.saturating_sub(height.saturating_sub(1)); }
}

// Use the same Unicode cell widths for chrome, the editor, and rendered documents.
fn cell_width(c: char) -> usize { UnicodeWidthChar::width(c).unwrap_or(1) }
fn column_width(text: &str, col: usize) -> usize {
    markdown::display_width(&text.chars().take(col).map(|c| if c.is_control() { ' ' } else { c }).collect::<String>())
}
fn fit(s: &str, width: usize) -> String {
    let safe: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut text = String::new();
    let mut used = 0;
    for g in safe.graphemes(true) {
        let cells = markdown::display_width(g);
        if used + cells > width { break; }
        if cells != 0 || used > 0 { text.push_str(g); }
        used += cells;
    }
    text.push_str(&" ".repeat(width - used));
    text
}
fn skip_cells(s: &str, skip: usize) -> String {
    let safe: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let mut used = 0;
    let mut out = String::new();
    for g in safe.graphemes(true) {
        let cells = markdown::display_width(g);
        if used >= skip { out.push_str(g); }
        else if used + cells > skip { out.push_str(&" ".repeat(used + cells - skip)); }
        used += cells;
    }
    out
}

fn paint(out: &mut String, row: usize, col: usize, width: usize, text: &str, style: &str) {
    if width > 0 { out.push_str(&format!("\x1b[{};{}H{}{}", row + 1, col + 1, style, fit(text, width))); }
}

fn main() {
    let arg = env::args_os().nth(1);
    if arg.as_deref().is_some_and(|value| value == "--version" || value == "-V") {
        println!("mdterm {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let result = run(arg.map(PathBuf::from));
    if let Err(err) = result { eprintln!("mdterm: {err}"); process::exit(1); }
}
fn run(path: Option<PathBuf>) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() { return Err(io::Error::new(io::ErrorKind::Other, "run mdterm in a terminal")); }
    let _terminal = Terminal::enter()?;
    let mut app = App::new(path)?;
    let mut size = os::size();
    app.draw()?;
    while app.running {
        let key = _terminal.read_key()?;
        let new_size = os::size();
        if key != Key::Unknown {
            if let Err(error) = app.handle(key) { app.prompt = Some(Prompt::Message(error.to_string())); }
        }
        if key != Key::Unknown || new_size != size { app.draw()?; }
        size = new_size;
    }
    Ok(())
}
