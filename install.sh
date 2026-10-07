#!/bin/sh
# timewent installer: downloads the latest release, puts it in Applications and opens it.
#   curl -fsSL https://raw.githubusercontent.com/GDenizKaratas/timewent/main/install.sh | sh
set -e

REPO="GDenizKaratas/timewent"
URL="${TIMEWENT_URL:-https://github.com/$REPO/releases/latest/download/timewent-macos-universal.zip}"

if [ "$(uname)" != "Darwin" ]; then
  echo "timewent runs on macOS only (for now)." >&2
  exit 1
fi

DEST="${TIMEWENT_DEST:-/Applications}"
if [ ! -w "$DEST" ]; then
  DEST="$HOME/Applications"
  mkdir -p "$DEST"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "→ downloading timewent…"
curl -fsSL "$URL" -o "$TMP/timewent.zip"
ditto -x -k "$TMP/timewent.zip" "$TMP"

if [ -d "$DEST/timewent.app" ]; then
  echo "→ replacing the installed version"
  osascript -e 'quit app "timewent"' >/dev/null 2>&1 || true
  sleep 1
  rm -rf "$DEST/timewent.app"
fi

mv "$TMP/timewent.app" "$DEST/"
# Not notarized yet: clear the download flag so macOS doesn't block the first launch.
xattr -dr com.apple.quarantine "$DEST/timewent.app" 2>/dev/null || true

echo "→ installed to $DEST/timewent.app"
[ -n "$TIMEWENT_NO_OPEN" ] || open "$DEST/timewent.app"
echo "✓ done. Press ▶ in the pill, then allow Accessibility when macOS asks."
echo "  Peek from anywhere with ⌥⇧Space."
