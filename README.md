# Batates

A desktop pet, written in Rust with Bevy.

## Platform support

| Platform | Status | Interaction |
|---|---|---|
| macOS | Working | Full |
| Windows | Builds and is lint-clean; not yet run on hardware | Full |
| Linux (Wayland) | Working on a single output, at scale 1 | Pet-only |
| Linux (X11) | Not supported | |

Interaction comes in two tiers, because the platforms genuinely differ:

- **Full** - clicking bare desktop sends the nearest pet walking there, as well
  as hovering, clicking, dragging and double-clicking the pet itself. This needs
  the global cursor position, which macOS and Windows provide.
- **Pet-only** - interaction with the pet itself, plus autonomous wandering.
  Wayland cannot report the pointer outside your own surface, so click-to-summon
  is impossible there by design, not by omission.

### Why GNOME is unsupported

An overlay must sit above other windows, cover the screen, and pass clicks
through except over the pet. Core Wayland permits none of those: a client cannot
raise or position itself, and cannot see the pointer outside its own surface.
The only protocol that allows it is `zwlr_layer_shell_v1`, which GNOME/Mutter
has declined to implement since 2019.

Batates checks for the protocol at startup and exits with an explanation rather
than failing obscurely. Known-working compositors: Sway, Hyprland, river, niri,
KDE Plasma 6 (KWin), COSMIC.

## Running

```sh
cargo run
```

Quit from the tray icon, with `batates --quit`, or with Ctrl-C. Only one
instance runs at a time; a second launch refuses and tells you so.

## Configuration

Optional. Without a config file the defaults apply. See `config.example.toml`
for every option, and copy it to:

- macOS: `~/Library/Application Support/com.batates.batates/config.toml`
- Linux: `~/.config/batates/config.toml`
- Windows: `%APPDATA%\batates\batates\config\config.toml`

Or point at any file with `$BATATES_CONFIG`.

An unknown key or an invalid value is an error with a line number, not a
silently ignored setting.

### Debugging interaction

If clicking the pet does not work, turn on the overlay:

```toml
[debug]
overlay = true
```

Green is the clickable box, red is where the app believes your cursor is. A gap
between the crosshair and your real pointer is a coordinate bug.

## Skins

A skin is a directory holding `sheet.png` and `skin.ron`. The sheet is a strict
`rows x columns` grid where the row is the state, in the order listed in
`skin.ron`. Frame counts, frame rate, sprite size, walk speed, state durations
and transition weights all come from the manifest, so a skin needs no code.

The koala is built into the binary. User skins live beside the config, in
`skins/<name>/`, and are selected with `skin = "<name>"`.

Build one from a folder of `<state>_<frame>.png` files:

```sh
python3 scripts/make_sprite.py assets/koala --out assets/skins/koala
```

Every state must be present, and every state must have at least one transition
out of it - a skin that could trap the pet is rejected at load.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo fmt --all
```

The codebase splits into pure logic and plumbing:

- `src/core/` - gameplay as pure functions. No windowing, no OS calls, no
  wall-clock time, no `cfg(target_os)`. This is what the tests cover.
- `src/platform/` - the window and the pointer, one backend per platform.
- `src/pet.rs` - Bevy systems moving data between the two.
- `src/config/`, `src/skin/` - parsing and validating files into typed values.

## Known issues

- The Wayland overlay only spans the first output it binds to; a second
  monitor is neither covered nor rendered onto.
- The Wayland overlay does not react to monitor hotplug or resize: its surface
  is sized once at startup.
- The Wayland overlay assumes `wl_output` scale 1. A fractionally- or
  integer-scaled display will render the pet at the wrong size relative to
  everything else on screen.
- The Wayland overlay's offscreen render target is always full output
  resolution, which can fail to allocate under GPU memory pressure (seen in
  practice with ~600 MiB of VRAM free on a 6 GiB card). A pet overlay does not
  need a full-resolution buffer; this is unoptimized, not fundamental.
- The Windows build is compile-checked but has not been run on real hardware.
- Releases are unsigned. macOS requires
  `xattr -dr com.apple.quarantine /Applications/Batates.app` on first launch.
