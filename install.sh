#!/bin/sh
# BitScout installer — downloads the latest release and sets up symlinks.
# Usage: curl -fsSL https://raw.githubusercontent.com/rainyulei/BitScout/main/install.sh | sh
set -e

REPO="rainyulei/BitScout"
INSTALL_DIR="${BITSCOUT_INSTALL_DIR:-$HOME/.bitscout/bin}"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *)       echo "Error: unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *)       echo "Error: unsupported architecture: $ARCH"; exit 1 ;;
    esac
    ;;
  *)
    echo "Error: unsupported OS: $OS"; exit 1
    ;;
esac

# Get latest release tag
echo "Detecting latest release..."
TAG=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$TAG" ]; then
  echo "Error: could not determine latest release"
  exit 1
fi

echo "Installing BitScout $TAG for $TARGET..."

# Download and extract
ARCHIVE="bitscout-${TAG}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"
TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading $URL..."
curl -fsSL "$URL" -o "$TMP_DIR/$ARCHIVE"

echo "Extracting..."
tar xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR"

# Install binary
mkdir -p "$INSTALL_DIR"
cp "$TMP_DIR/bitscout-${TAG}-${TARGET}/bitscout" "$INSTALL_DIR/bitscout"
chmod +x "$INSTALL_DIR/bitscout"

# Create symlinks (rg, grep, find, fd, cat)
for cmd in rg grep find fd cat; do
  ln -sf "$INSTALL_DIR/bitscout" "$INSTALL_DIR/$cmd"
done

echo ""
echo "Installed BitScout $TAG to $INSTALL_DIR"
echo ""

# Auto-configure PATH
SHELL_NAME=$(basename "$SHELL")
case "$SHELL_NAME" in
  zsh)  RC="$HOME/.zshrc" ;;
  bash) RC="$HOME/.bashrc" ;;
  *)    RC="" ;;
esac

PATH_LINE="export PATH=\"$INSTALL_DIR:\$PATH\""

case ":$PATH:" in
  *":$INSTALL_DIR:"*)
    echo "PATH already configured."
    ;;
  *)
    if [ -n "$RC" ] && [ -f "$RC" ]; then
      # Only add if not already in rc file
      if ! grep -qF "$INSTALL_DIR" "$RC" 2>/dev/null; then
        echo "" >> "$RC"
        echo "# BitScout" >> "$RC"
        echo "$PATH_LINE" >> "$RC"
        echo "PATH auto-configured in $RC"
      fi
    else
      echo "Add this to your shell profile:"
      echo "  $PATH_LINE"
    fi
    ;;
esac

echo ""
echo "Done! Open a new terminal window, then run: bitscout --help"
