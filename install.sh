#!/bin/sh
# Installs the latest NetUser release for x86_64 Linux.
#   curl -fsSL https://raw.githubusercontent.com/blackswanalpha/netuser/main/install.sh | sh
# Uses the .deb on apt systems, the .rpm on dnf/zypper systems, else the AppImage.
set -eu

REPO=blackswanalpha/netuser
arch=$(uname -m)
[ "$arch" = x86_64 ] || { echo "NetUser releases are x86_64 only (this is $arch)." >&2; exit 1; }

urls=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
  grep -o '"browser_download_url": *"[^"]*"' | cut -d'"' -f4)

pick() {
  url=$(printf '%s\n' "$urls" | grep -E "$1" | head -n1)
  [ -n "$url" ] || { echo "No $2 asset in the latest release." >&2; exit 1; }
  echo "$url"
}

tmp=$(mktemp -d)
chmod 755 "$tmp" # apt's sandbox user must be able to read the package
trap 'rm -rf "$tmp"' EXIT

if command -v apt-get >/dev/null 2>&1; then
  curl -fL -o "$tmp/netuser.deb" "$(pick '_amd64\.deb$' .deb)"
  sudo apt-get install -y "$tmp/netuser.deb"
elif command -v dnf >/dev/null 2>&1; then
  curl -fL -o "$tmp/netuser.rpm" "$(pick '\.x86_64\.rpm$' .rpm)"
  sudo dnf install -y "$tmp/netuser.rpm"
elif command -v zypper >/dev/null 2>&1; then
  curl -fL -o "$tmp/netuser.rpm" "$(pick '\.x86_64\.rpm$' .rpm)"
  sudo zypper --non-interactive install --allow-unsigned-rpm "$tmp/netuser.rpm"
else
  dest="$HOME/.local/bin/netuser"
  mkdir -p "$(dirname "$dest")"
  curl -fL -o "$dest" "$(pick '\.AppImage$' AppImage)"
  chmod +x "$dest"
  echo "Installed AppImage to $dest (needs libfuse2). Make sure ~/.local/bin is on PATH."
  exit 0
fi
echo "NetUser installed. Launch it from your app menu or run: netuser"
