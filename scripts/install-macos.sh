#!/usr/bin/env bash
# tilekit — macOS installer (Apple Silicon & Intel)
# Installs toolchain + skhd, builds the workspace, installs binaries,
# wires the cheatsheet hotkey, runs tests, launches the simulator.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN_DIR="$HOME/.local/bin"
say() { printf '\033[1;35m[tilekit]\033[0m %s\n' "$*"; }

# 1. Xcode command line tools (compiler backend for rustc)
if ! xcode-select -p >/dev/null 2>&1; then
  say "Installing Xcode Command Line Tools (a dialog will appear)..."
  xcode-select --install || true
  say "Re-run this script after the CLT install finishes."
  exit 0
fi

# 2. Homebrew
if ! command -v brew >/dev/null 2>&1; then
  say "Installing Homebrew..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  eval "$(/opt/homebrew/bin/brew shellenv 2>/dev/null || /usr/local/bin/brew shellenv)"
fi

# 3. Rust (rustup, not brew rust — you want rustup for cross targets later)
if ! command -v cargo >/dev/null 2>&1; then
  say "Installing Rust via rustup..."
  brew install rustup
  rustup-init -y --default-toolchain stable
  source "$HOME/.cargo/env"
fi

# 4. skhd — the hotkey daemon (keys stay out of the WM, per the architecture)
if ! command -v skhd >/dev/null 2>&1; then
  say "Installing skhd..."
  brew install koekeishiya/formulae/skhd
fi

# 5. Build + test
say "Building workspace (release)..."
cd "$REPO_DIR"
cargo build --release --workspace
say "Running the wm-core test suite..."
cargo test -p wm-core --quiet

# 6. Install binaries
mkdir -p "$BIN_DIR"
install -m 755 target/release/tilekit-cheatsheet "$BIN_DIR/"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "NOTE: add 'export PATH=\"$BIN_DIR:\$PATH\"' to your shell rc." ;;
esac

# 7. skhd config — merge our bindings if not already present
SKHD_RC="$HOME/.config/skhd/skhdrc"
mkdir -p "$(dirname "$SKHD_RC")"
if ! grep -q "tilekit" "$SKHD_RC" 2>/dev/null; then
  say "Appending tilekit bindings to $SKHD_RC"
  cat "$REPO_DIR/configs/skhdrc" >> "$SKHD_RC"
fi
skhd --restart-service 2>/dev/null || skhd --start-service

say "Done. Press ⌘⌥/ to open the cheatsheet."
say "Heads-up for the daemon phase: when wmd lands it will need"
say "Accessibility permission (System Settings → Privacy & Security)."
say "Launching the engine simulator now — 'open' adds windows, 'q' quits:"
exec cargo run -q --example simulate -p wm-core
