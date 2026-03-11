use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader},
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use rodio::{Decoder, OutputStream, Sink, Source};

// ─────────────────────────────────────────────────────────────
// Looping source — reopens the file when the decoder exhausts
// so looping works regardless of whether the decoder supports seek
// ─────────────────────────────────────────────────────────────

struct LoopingSound {
    path:        PathBuf,
    inner:       Decoder<BufReader<File>>,
    channels:    u16,
    sample_rate: u32,
}

impl LoopingSound {
    fn new(path: PathBuf) -> anyhow::Result<Self> {
        let dec = Self::open(&path)?;
        let channels    = dec.channels();
        let sample_rate = dec.sample_rate();
        Ok(Self { path, inner: dec, channels, sample_rate })
    }

    fn open(path: &Path) -> anyhow::Result<Decoder<BufReader<File>>> {
        Ok(Decoder::new(BufReader::new(File::open(path)?))?)
    }
}

impl Iterator for LoopingSound {
    type Item = <Decoder<BufReader<File>> as Iterator>::Item;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(s) = self.inner.next() { return Some(s); }
            match Self::open(&self.path) {
                Ok(dec) => self.inner = dec,
                Err(_)  => return None,
            }
        }
    }
}

impl Source for LoopingSound {
    fn current_frame_len(&self) -> Option<usize> { self.inner.current_frame_len() }
    fn channels(&self)          -> u16            { self.channels }
    fn sample_rate(&self)       -> u32            { self.sample_rate }
    fn total_duration(&self)    -> Option<Duration> { None }
}
use serde::{Deserialize, Serialize};

// ── White on black palette ────────────────────────────────────
const HOT:       Color = Color::Rgb(255, 255, 255);
const BRIGHT:    Color = Color::Rgb(220, 220, 220);
const MID:       Color = Color::Rgb(160, 160, 160);
const DIM:       Color = Color::Rgb( 90,  90,  90);
const BG:        Color = Color::Rgb(  0,   0,   0);
const BG2:       Color = Color::Rgb( 10,  10,  10);
const CURSOR_BG: Color = Color::Rgb( 40,  40,  40);
const BORDER_C:  Color = Color::Rgb(100, 100, 100);

// ─────────────────────────────────────────────────────────────
// Model
// ─────────────────────────────────────────────────────────────

struct Sound {
    name:   String,
    volume: f32,
    active: bool,
    sink:   Sink,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct SoundState { volume: f32, active: bool }

#[derive(Serialize, Deserialize, Clone, Default)]
struct Preset { master: f32, sounds: HashMap<String, SoundState> }

#[derive(PartialEq, Clone, Copy)]
enum Panel { Sounds, Presets, Input }

struct App {
    sounds:        Vec<Sound>,
    cursor:        usize,
    master:        f32,
    presets:       HashMap<String, Preset>,
    preset_names:  Vec<String>,
    preset_cursor: usize,
    preset_input:  String,
    panel:         Panel,
    _stream:       OutputStream,   // must stay alive
}

// ─────────────────────────────────────────────────────────────
// App logic
// ─────────────────────────────────────────────────────────────

impl App {
    fn new() -> anyhow::Result<Self> {
        let (_stream, handle) = OutputStream::try_default()?;

        let sounds_dir = murmur_dir().join("sounds");
        let mut entries: Vec<_> = std::fs::read_dir(&sounds_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |x| x == "ogg"))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        let mut sounds = Vec::new();
        for entry in &entries {
            let path = entry.path();
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let sink = Sink::try_new(&handle)?;
            sink.append(LoopingSound::new(path.clone())?);
            sink.pause();
            sink.set_volume(0.0);
            sounds.push(Sound { name, volume: 1.0, active: false, sink });
        }

        let presets      = load_presets();
        let preset_names = sorted_keys(&presets);
        let last         = load_last();

        let mut app = Self {
            sounds,
            cursor: 0,
            master: 1.0,
            presets,
            preset_names,
            preset_cursor: 0,
            preset_input: String::new(),
            panel: Panel::Sounds,
            _stream,
        };

        if let Some(name) = last {
            if let Some(idx) = app.preset_names.iter().position(|n| n == &name) {
                app.preset_cursor = idx;
            }
            app.apply_preset(&name);
        }

        Ok(app)
    }

    fn effective_vol(&self, i: usize) -> f32 {
        self.sounds[i].volume * self.master
    }

    fn sync_sink(&self, i: usize) {
        let s = &self.sounds[i];
        if s.active {
            s.sink.set_volume(self.effective_vol(i));
            s.sink.play();
        } else {
            s.sink.pause();
            s.sink.set_volume(0.0);
        }
    }

    fn toggle(&mut self) {
        let i = self.cursor;
        self.sounds[i].active ^= true;
        self.sync_sink(i);
    }

    fn vol_adjust(&mut self, delta: f32) {
        let i = self.cursor;
        self.sounds[i].volume = (self.sounds[i].volume + delta).clamp(0.0, 1.0);
        if self.sounds[i].active { self.sync_sink(i); }
    }

    fn master_adjust(&mut self, delta: f32) {
        self.master = (self.master + delta).clamp(0.0, 1.0);
        for i in 0..self.sounds.len() {
            if self.sounds[i].active { self.sync_sink(i); }
        }
    }

    fn stop_all(&mut self) {
        for s in &mut self.sounds {
            s.active = false;
            s.sink.pause();
            s.sink.set_volume(0.0);
        }
    }

    fn save_preset(&mut self) {
        let name = self.preset_input.trim().to_string();
        if name.is_empty() { return; }
        self.presets.insert(name, Preset {
            master: self.master,
            sounds: self.sounds.iter()
                .map(|s| (s.name.clone(), SoundState { volume: s.volume, active: s.active }))
                .collect(),
        });
        self.preset_names = sorted_keys(&self.presets);
        self.preset_input.clear();
        save_presets(&self.presets);
    }

    fn load_preset(&mut self) {
        let Some(name) = self.preset_names.get(self.preset_cursor).cloned() else { return };
        self.apply_preset(&name);
        save_last(&name);
    }

    fn apply_preset(&mut self, name: &str) {
        let Some(preset) = self.presets.get(name).cloned() else { return };
        self.master = preset.master;
        for i in 0..self.sounds.len() {
            if let Some(st) = preset.sounds.get(&self.sounds[i].name) {
                self.sounds[i].volume = st.volume;
                self.sounds[i].active = st.active;
            }
            self.sync_sink(i);
        }
    }

    fn delete_preset(&mut self) {
        let Some(name) = self.preset_names.get(self.preset_cursor).cloned() else { return };
        self.presets.remove(&name);
        self.preset_names = sorted_keys(&self.presets);
        if !self.preset_names.is_empty() {
            self.preset_cursor = self.preset_cursor.min(self.preset_names.len() - 1);
        } else {
            self.preset_cursor = 0;
        }
        save_presets(&self.presets);
    }
}

fn sorted_keys(m: &HashMap<String, Preset>) -> Vec<String> {
    let mut v: Vec<_> = m.keys().cloned().collect();
    v.sort();
    v
}

// ─────────────────────────────────────────────────────────────
// Persistence
// ─────────────────────────────────────────────────────────────

fn murmur_dir() -> PathBuf {
    let mut p = dirs::home_dir().unwrap_or_default();
    p.push(".config/murmur");
    p
}

fn config_path() -> PathBuf {
    murmur_dir().join("presets.json")
}

fn load_presets() -> HashMap<String, Preset> {
    let p = config_path();
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_presets(presets: &HashMap<String, Preset>) {
    let p = config_path();
    if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
    if let Ok(s) = serde_json::to_string_pretty(presets) { let _ = std::fs::write(p, s); }
}

fn last_path() -> PathBuf { murmur_dir().join("last") }

fn save_last(name: &str) {
    let _ = std::fs::write(last_path(), name);
}

fn load_last() -> Option<String> {
    std::fs::read_to_string(last_path()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ─────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────

fn vol_bar(volume: f32, width: usize) -> String {
    let filled = (width as f32 * volume).round() as usize;
    let empty  = width.saturating_sub(filled);
    format!("[{}{}]{:4.0}%", "█".repeat(filled), "░".repeat(empty), volume * 100.0)
}

fn panel_block(title: &str, active: bool) -> Block<'_> {
    let (border_col, title_style) = if active {
        (HOT, Style::default().fg(BG).bg(HOT).add_modifier(Modifier::BOLD))
    } else {
        (BORDER_C, Style::default().fg(MID))
    };
    Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(border_col))
        .title_style(title_style)
        .style(Style::default().bg(BG))
}

fn ui(f: &mut Frame, app: &App) {
    let area = f.area();

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(2, 3), Constraint::Ratio(1, 3)])
        .split(rows[0]);

    render_sounds(f, app, cols[0]);
    render_presets(f, app, cols[1]);
    render_master(f, app, rows[1]);
    render_footer(f, rows[2]);
}

fn render_sounds(f: &mut Frame, app: &App, area: Rect) {
    let block = panel_block("SOUNDS", app.panel == Panel::Sounds);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // " [ ON]  name-14-chars  [bar]  vol%"
    // fixed chars: 1+5+2+14+2 = 24, suffix "] xxx%" = 6  → bar_w = w - 30
    let bar_w = (inner.width as usize).saturating_sub(30).max(4);

    let lines: Vec<Line> = app.sounds.iter().enumerate().map(|(i, s)| {
        let tag   = if s.active { "[ ON]" } else { "[   ]" };
        let label = format!("{:<14}", s.name.replace('_', " "));
        let bar   = vol_bar(s.volume, bar_w);
        let text  = format!(" {}  {}  {}", tag, label, bar);

        match (i == app.cursor, app.panel == Panel::Sounds, s.active) {
            (true, true,  _)     => Line::styled(text, Style::default().fg(HOT).bg(CURSOR_BG).add_modifier(Modifier::BOLD)),
            (true, false, _)     => Line::styled(text, Style::default().fg(DIM).add_modifier(Modifier::UNDERLINED)),
            (false, _, true)     => Line::styled(text, Style::default().fg(BRIGHT)),
            (false, _, false)    => Line::styled(text, Style::default().fg(DIM)),
        }
    }).collect();

    f.render_widget(Paragraph::new(lines).style(Style::default().bg(BG)), inner);
}

fn render_presets(f: &mut Frame, app: &App, area: Rect) {
    let preset_active = matches!(app.panel, Panel::Presets | Panel::Input);
    let block = panel_block("PRESETS", preset_active);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),       // list
            Constraint::Length(1),    // "name:" label
            Constraint::Length(1),    // input
            Constraint::Length(1),    // hints
        ])
        .split(inner);

    // Preset list
    let preset_focused = matches!(app.panel, Panel::Presets | Panel::Input);
    let items: Vec<ListItem> = app.preset_names.iter().enumerate().map(|(i, name)| {
        let prefix = if i == app.preset_cursor { "> " } else { "  " };
        let text = format!("{}{}", prefix, name);
        if i == app.preset_cursor && preset_focused {
            ListItem::new(text).style(Style::default().fg(HOT).bg(CURSOR_BG).add_modifier(Modifier::BOLD))
        } else {
            ListItem::new(text).style(Style::default().fg(DIM))
        }
    }).collect();
    f.render_widget(List::new(items).style(Style::default().bg(BG2)), sections[0]);

    // Label
    f.render_widget(
        Paragraph::new(" name:").style(Style::default().fg(MID)),
        sections[1],
    );

    // Input
    let (input_text, input_style) = if app.panel == Panel::Input {
        (format!(" {}_", app.preset_input), Style::default().fg(HOT))
    } else {
        (format!(" {}", app.preset_input), Style::default().fg(DIM))
    };
    f.render_widget(Paragraph::new(input_text).style(input_style), sections[2]);

    // Hints
    f.render_widget(
        Paragraph::new(" F2 save  F3 load  F4 del").style(Style::default().fg(MID)),
        sections[3],
    );
}

fn render_master(f: &mut Frame, app: &App, area: Rect) {
    // " MASTER  [bar] xxx%   m/M:vol  F5:stop"
    // " MASTER  [" = 10, "] xxx%" = 6, "   m/M:vol  F5:stop" = 19 → bar_w = w - 35
    let bar_w  = (area.width as usize).saturating_sub(35).max(4);
    let bar    = vol_bar(app.master, bar_w);
    let hint   = "   m/M:master  F5:stop";
    let text   = format!(" MASTER  {}{}",  bar, hint);
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(MID).bg(BG2)),
        area,
    );
}

fn render_footer(f: &mut Frame, area: Rect) {
    let text = " ↑↓/jk:select  SPC:toggle  ←→:vol  Shift+←→:fine  Tab:panel  i:name  Enter:load  q:quit";
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(MID).bg(BG2)),
        area,
    );
}

// ─────────────────────────────────────────────────────────────
// Input handling
// ─────────────────────────────────────────────────────────────

/// Returns true when the app should exit.
fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let ctrl  = key.modifiers.contains(KeyModifiers::CONTROL);

    // Always-on keys
    match key.code {
        KeyCode::Char('c') if ctrl                        => return true,
        KeyCode::Char('q') if app.panel != Panel::Input   => return true,
        KeyCode::F(2)                                      => { app.save_preset();   return false; }
        KeyCode::F(3)                                      => { app.load_preset();   return false; }
        KeyCode::F(4)                                      => { app.delete_preset(); return false; }
        KeyCode::F(5)                                      => { app.stop_all();      return false; }
        KeyCode::Tab => {
            app.panel = match app.panel {
                Panel::Sounds           => Panel::Presets,
                Panel::Presets
                | Panel::Input          => Panel::Sounds,
            };
            return false;
        }
        // m/M adjust master from any panel except while typing
        KeyCode::Char('m') if app.panel != Panel::Input => { app.master_adjust(-0.05); return false; }
        KeyCode::Char('M') if app.panel != Panel::Input => { app.master_adjust( 0.05); return false; }
        _ => {}
    }

    // Panel-specific keys
    match app.panel {
        Panel::Sounds => match key.code {
            KeyCode::Up   | KeyCode::Char('k') => { app.cursor = app.cursor.saturating_sub(1); }
            KeyCode::Down | KeyCode::Char('j') => { app.cursor = (app.cursor + 1).min(app.sounds.len().saturating_sub(1)); }
            KeyCode::Char(' ')                 => app.toggle(),
            KeyCode::Left                      => app.vol_adjust(if shift { -0.01 } else { -0.05 }),
            KeyCode::Right                     => app.vol_adjust(if shift {  0.01 } else {  0.05 }),
            _ => {}
        }
        Panel::Presets => match key.code {
            KeyCode::Up   | KeyCode::Char('k') => { app.preset_cursor = app.preset_cursor.saturating_sub(1); }
            KeyCode::Down | KeyCode::Char('j') => { app.preset_cursor = (app.preset_cursor + 1).min(app.preset_names.len().saturating_sub(1)); }
            KeyCode::Enter                     => app.load_preset(),
            KeyCode::Char('i') | KeyCode::Char('n') => { app.panel = Panel::Input; }
            _ => {}
        }
        Panel::Input => match key.code {
            KeyCode::Enter     => { app.save_preset(); app.panel = Panel::Presets; }
            KeyCode::Esc       => { app.preset_input.clear(); app.panel = Panel::Presets; }
            KeyCode::Backspace => { app.preset_input.pop(); }
            KeyCode::Char(c)   => { app.preset_input.push(c); }
            _ => {}
        }
    }

    false
}

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    let mut app = App::new()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.clear()?;

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> anyhow::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;
        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(app, key) { break; }
            }
        }
    }
    Ok(())
}
