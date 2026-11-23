#!/usr/bin/env bash
# tilekit — Linux installer (Debian/Ubuntu/Raspberry Pi OS, Fedora, Arch)
# ARM64-safe: everything here builds natively on a Pi 5.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
say() { printf '\033[1;32m[tilekit]\033[0m %s\n' "$*"; }
SUDO=""; [ "$(id -u)" -ne 0 ] && SUDO="sudo"

# 1. Distro packages: compiler, hotkey daemon, opener
if command -v apt-get >/dev/null 2>&1; then
  say "Installing deps via apt..."
  $SUDO apt-get update -qq
  $SUDO apt-get install -y -qq build-essential curl sxhkd xdg-utils
elif command -v dnf >/dev/null 2>&1; then
  say "Installing deps via dnf..."
  $SUDO dnf install -y gcc make curl sxhkd xdg-utils
elif command -v pacman >/dev/null 2>&1; then
  say "Installing deps via pacman..."
  $SUDO pacman -Sy --needed --noconfirm base-devel curl sxhkd xdg-utils
else
  say "Unknown distro — install a C toolchain, sxhkd, and xdg-utils manually."
fi

# 2. Rust via rustup (distro rustc is often too old; rustup also gives
#    you cross targets for the rest of the fleet)
if ! command -v cargo >/dev/null 2>&1; then
  say "Installing Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
fi

# 3. Build + test
say "Building workspace (release)..."
cd "$REPO_DIR"
cargo build --release --workspace
say "Running the wm-core test suite..."
cargo test -p wm-core --quiet

# 4. Install binaries
mkdir -p "$BIN_DIR"
install -m 755 target/release/tilekit-cheatsheet "$BIN_DIR/"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "NOTE: add 'export PATH=\"$BIN_DIR:\$PATH\"' to your shell rc." ;;
esac

# 5. sxhkd config — merge our bindings if not already present
SXHKD_RC="$HOME/.config/sxhkd/sxhkdrc"
mkdir -p "$(dirname "$SXHKD_RC")"
if ! grep -q "tilekit" "$SXHKD_RC" 2>/dev/null; then
  say "Appending tilekit bindings to $SXHKD_RC"
  cat "$REPO_DIR/configs/sxhkdrc" >> "$SXHKD_RC"
fi
if pgrep -x sxhkd >/dev/null 2>&1; then
  pkill -USR1 -x sxhkd
  say "sxhkd reloaded."
else
  say "Start sxhkd in your X session (add 'sxhkd &' to ~/.xinitrc or your"
  say "session autostart). Headless Pi nodes can skip this."
fi

say "Done. Super+Shift+/ opens the cheatsheet (in a graphical session)."
say "Launching the engine simulator now — 'open' adds windows, 'q' quits:"
exec cargo run -q --example simulate -p wm-core
