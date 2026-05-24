# Myari

Myari is a small Rust/Bevy hex-map strategy prototype with procedural “team islands” world generation.

## Run

From `c:\Users\micah\Myari`:

```bash
cargo run
```

## Controls

- Left-drag: Pan camera
- Mouse wheel: Zoom
- Hover: Highlight hex and show terrain label
- Right-click: Select hex
- M: Move selected player scout to hovered adjacent hex (consumes 1 move)
- Enter: End turn
- R: Reroll world (new seed)
- Esc: Settings menu (grid toggle)
- F5: Save
- F9: Load

## Notes

- Map radius comes from `src/map.rs` (`MAP_RADIUS`).
- Saves are written to `%LOCALAPPDATA%\\Myari\\save.json` (fallback `%APPDATA%`).
