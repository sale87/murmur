# murmur

Ambient sound mixer for the terminal. Play multiple looping OGG files simultaneously with individual and master volume control. Save and load named presets.

## Usage

```
cargo run --release
```

Sounds are loaded from the `sounds/` directory. Add `.ogg` files there to extend the library.

## Keys

| Key | Action |
|-----|--------|
| `↑` `↓` / `j` `k` | Move cursor |
| `Space` | Toggle sound on/off |
| `←` `→` | Volume ±5% |
| `Shift+←` `Shift+→` | Volume ±1% |
| `m` / `M` | Master volume ±5% |
| `Tab` | Switch panel (Sounds ↔ Presets) |
| `i` | Enter preset name |
| `Enter` | Load selected preset |
| `Esc` | Cancel input |
| `F2` | Save preset |
| `F3` | Load preset |
| `F4` | Delete preset |
| `F5` | Stop all |
| `q` / `Ctrl+C` | Quit |

## Presets

Presets are stored in `~/.config/murmur/presets.json`.

## Dependencies

- [ratatui](https://github.com/ratatui-org/ratatui) — TUI
- [rodio](https://github.com/RustAudio/rodio) — audio playback
- [crossterm](https://github.com/crossterm-rs/crossterm) — terminal input
