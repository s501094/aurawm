# AuraWM (tilekit)

A cross-platform tiling window manager. Windows backend is live
(multi-monitor, DPI-aware); the pure engine underneath it is Zero
dependencies, zero OS calls, zero I/O: commands and platform events in,
`Effect` lists out. This is the crate every platform backend, the Lua
host, and the future Wayland compositor consume.


## Start / stop (Windows testing workflow)

    powershell -ExecutionPolicy Bypass -File scripts\start-aurawm.ps1
    powershell -ExecutionPolicy Bypass -File scripts\stop-aurawm.ps1 -StartGlazeWM

start-aurawm: checks deps (prompts to install missing ones), STOPS
GlazeWM, builds if needed, starts whkd + wmd with debug logs in
%LOCALAPPDATA%\aurawm\logs. Pass -Rebuild after pulling new code.
stop-aurawm: graceful wm-exit (restores every hidden window), stops
whkd, and -StartGlazeWM hands control back to GlazeWM.

Telemetry: RUST_LOG=wmd=debug,wm_core=debug (default from the start
script). Attach the newest log from %LOCALAPPDATA%\aurawm\logs when
reporting problems.

## GlazeWM command parity

| GlazeWM                      | AuraWM              | Notes |
|------------------------------|---------------------|-------|
| focus --direction            | YES same syntax     | |
| focus --next/prev-workspace  | YES same syntax     | |
| focus --recent-workspace     | YES same syntax     | |
| focus --workspace N          | YES same syntax     | |
| move --direction             | YES same syntax     | |
| move --next/prev/recent-ws   | YES same syntax     | |
| move --workspace N           | YES same syntax     | |
| close                        | YES                 | |
| toggle-floating              | YES                 | |
| toggle-fullscreen            | YES                 | v0: sized over monitor, z-order not forced |
| toggle-minimized             | YES                 | taskbar restore re-adopts automatically |
| toggle-tiling-direction      | YES (=toggle-split) | |
| wm-exit                      | YES                 | restores all hidden windows first |
| wm-redraw                    | YES (=retile)       | |
| resize --width/--height      | PARTIAL             | resize --direction --amount today |
| size --width/--height        | NOT YET             | |
| move-workspace --direction   | NOT YET             | move ws between monitors |
| wm-cycle-focus               | NOT YET             | tiling->floating->fullscreen cycle |
| ignore                       | NOT YET             | needs Lua window rules |
| adjust-borders               | NOT YET             | overlay milestone |
| binding modes                | N/A                 | whkd's job, not the WM's |
| shell-exec                   | N/A                 | whkd runs commands natively |
| wm-reload-config             | NOT YET             | arrives with the Lua layer |

## Install

One script per OS: installs the toolchain (rustup) and the hotkey daemon,
builds the workspace, runs the test suite, installs binaries to
`~/.local/bin`, wires the keybindings, and launches the engine simulator.

**Windows** (PowerShell):

    powershell -ExecutionPolicy Bypass -File scripts\install-windows.ps1

Installs rustup + whkd via winget. If rustup reports missing MSVC Build
Tools, let its prompt install them, then re-run.

**macOS** (Apple Silicon or Intel):

    ./scripts/install-macos.sh

Installs Xcode CLT (prompts once — re-run after), Homebrew if missing,
rustup, and skhd. Cheatsheet hotkey: Cmd+Opt+/

**Linux** (Debian/Ubuntu/Raspberry Pi OS, Fedora, Arch — ARM64 OK):

    ./scripts/install-linux.sh

Installs build tools + sxhkd via your package manager, rustup, and adds
`sxhkd &` guidance for your X session. Cheatsheet hotkey: Super+Shift+/

**Manual / no hotkey daemon** (any OS with Rust installed):

    cargo test -p wm-core                 # 17 unit tests
    cargo run --example simulate -p wm-core   # interactive engine REPL
    cargo build --release -p wm-cheatsheet    # then run tilekit-cheatsheet

Status: on WINDOWS the install script launches wmd, the real tiling
daemon -- windows tile, whkd keybindings are live. On macOS/Linux the
daemon builds and serves IPC but runs a state-only null backend until
those OS backends land, so the scripts launch the engine simulator.

## Layout

    wm-core/src/
      geometry.rs    Rect, gap-aware split math, directions
      tree.rs        BSP tree: insert / collapse-on-remove / swap /
                     toggle-split / nearest-ancestor resize (bspwm style)
      workspace.rs   tree + floating set + geometric focus navigation
      command.rs     wmctl grammar parser (flag + positional forms;
                     unknown verbs fall through as Custom for Lua)
      state.rs       WmState reducer: 9 workspaces, Effect emission
      lib.rs         re-exports + 17-test suite
    wm-core/examples/
      simulate.rs    interactive ASCII simulator (no OS backend needed)

## Test / drive it

    cargo test                       # 17 unit tests
    cargo run --example simulate     # REPL: open, close, focus left,
                                     # move right, resize down --amount 0.1,
                                     # workspace 2, send 3, toggle-float,
                                     # toggle-split, call <lua-verb> ...

## Design decisions baked in

- Directional focus is geometric (rect centers + perpendicular-overlap
  preference), not tree-order — matches GlazeWM/i3 feel.
- Resize adjusts the nearest ancestor split with a matching axis
  (bspwm semantics); ratio clamped to [0.1, 0.9].
- Auto-orientation on insert: wide focused rect splits side-by-side,
  tall splits stacked.
- `Focus(dir)` at a screen edge emits zero effects — the daemon
  reports that to Lua so `focus-or-wrap` style plugins can act.
- Workspace switches emit Hide/Show effect batches; backends map these
  to SW_HIDE / AX minimize-offscreen / unmap respectively.
- Unknown wmctl verbs parse into `Command::Custom` → `Effect::Lua`,
  which is the hook the embedded Lua command table consumes.

## Also in this repo

- wm-cheatsheet/  — `tilekit-cheatsheet` binary; embeds the OS-matched
  Catppuccin cheatsheet (windows/macos/linux.html) at compile time
- scripts/        — install-windows.ps1, install-macos.sh, install-linux.sh:
  toolchain + hotkey daemon install, build, test, hotkey wiring, launch
- configs/        — whkdrc / skhdrc / sxhkdrc keybinding files

## Next (in build order)

1. wm-lua (mlua, vendored 5.4) — custom verbs + window rules
2. wm-overlay — custom borders/indicators
3. macOS (AX) and Linux (X11) backends for wmd
4. Multi-monitor; named pipe/UDS transport (replacing localhost TCP)
