#!/bin/sh
# Checks the build prerequisites and says what to install instead of failing deep inside tauri.
missing=0
if ! command -v cargo >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    echo "timewent: Rust is installed but this terminal doesn't see it yet."
    echo "          Open a new terminal, or run:  source \"\$HOME/.cargo/env\""
  else
    echo "timewent: Rust is missing. Install it with:"
    echo "          curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "          then open a new terminal."
  fi
  missing=1
fi
if ! xcode-select -p >/dev/null 2>&1; then
  echo "timewent: Xcode Command Line Tools are missing. Install them with:  xcode-select --install"
  missing=1
fi
exit $missing
