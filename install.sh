#!/bin/sh
# Installs the latest NetUser release for x86_64 Linux.
#   curl -fsSL https://raw.githubusercontent.com/blackswanalpha/netuser/main/install.sh | sh
# Uses the .deb on apt systems, the .rpm on dnf/zypper systems, else the AppImage.
set -eu

REPO=blackswanalpha/netuser
arch=$(uname -m)
[ "$arch" = x86_64 ] || { echo "NetUser releases are x86_64 only (this is $arch)." >&2; exit 1; }

# Resolve the latest tag from the releases/latest redirect. Avoids the GitHub
# API, which allows only 60 unauthenticated calls per hour per IP.
tag=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")
tag=${tag##*/}
ver=${tag#v}
case "$ver" in
  [0-9]*) ;;
  *) echo "Couldn't find the latest NetUser release." >&2; exit 1 ;;
esac
base="https://github.com/$REPO/releases/download/$tag"

tmp=$(mktemp -d)
chmod 755 "$tmp" # apt's sandbox user must be able to read the package
trap 'rm -rf "$tmp"' EXIT

if command -v apt-get >/dev/null 2>&1; then
  curl -fL -o "$tmp/netuser.deb" "$base/NetUser_${ver}_amd64.deb"
  sudo apt-get install -y "$tmp/netuser.deb"
elif command -v dnf >/dev/null 2>&1; then
  curl -fL -o "$tmp/netuser.rpm" "$base/NetUser-${ver}-1.x86_64.rpm"
  sudo dnf install -y "$tmp/netuser.rpm"
elif command -v zypper >/dev/null 2>&1; then
  curl -fL -o "$tmp/netuser.rpm" "$base/NetUser-${ver}-1.x86_64.rpm"
  sudo zypper --non-interactive install --allow-unsigned-rpm "$tmp/netuser.rpm"
else
  dest="$HOME/.local/bin/netuser"
  mkdir -p "$(dirname "$dest")"
  curl -fL -o "$dest" "$base/NetUser_${ver}_amd64.AppImage"
  chmod +x "$dest"
  echo "Installed AppImage to $dest (needs libfuse2). Make sure ~/.local/bin is on PATH."
  exit 0
fi
echo "NetUser installed. Launch it from your app menu or run: netuser"
