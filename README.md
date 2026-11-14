<p align="center"><img src="assets/logo.svg" width="96" alt="NetUser logo"></p>

# NetUser

A Linux desktop app that shows which devices are on your WiFi. Name devices, see who is online or recently away, and check live signal strength and throughput. No root required.

## Install

One line (x86_64; picks .deb, .rpm or AppImage for your distro):

```sh
curl -fsSL https://raw.githubusercontent.com/blackswanalpha/netuser/main/install.sh | sh
```

Or download a package from [Releases](https://github.com/blackswanalpha/netuser/releases/latest):

| Distro | Command |
|---|---|
| Debian, Ubuntu, Mint | `sudo apt install ./NetUser_0.1.0_amd64.deb` |
| Fedora, RHEL | `sudo dnf install ./NetUser-0.1.0-1.x86_64.rpm` |
| Any (AppImage, needs libfuse2) | `chmod +x NetUser_0.1.0_amd64.AppImage && ./NetUser_0.1.0_amd64.AppImage` |

Remove with `sudo apt remove net-user` or `sudo dnf remove net-user`.

## Runtime dependencies

| Tool | Package | Needed for |
|---|---|---|
| `ip` | iproute2 | Device scan (required) |
| `xdg-open` | xdg-utils | Opening the router page |
| `nmcli` | network-manager | SSID, link rate, band, channel |
| `/usr/share/nmap/nmap-mac-prefixes` | nmap-common (or nmap) | Device vendor names |
| `wl-copy` or `xclip` | wl-clipboard or xclip | Copying a MAC for blocking |

The .deb pulls in the required ones and recommends the rest.

## How it works

- **Scan:** sends one UDP datagram to every address in your subnet (capped at /22) so the kernel ARPs for each host, then reads the neighbour table with `ip -j neigh`. Any neighbour with a known MAC counts as present. Devices missed in one scan stay listed as "Away" for 3 minutes.
- **Network:** signal from `/proc/net/wireless`, link details from `nmcli`, throughput from the interface byte counters. Download/Upload are current traffic, not a speed test.
- **Names** are stored per MAC in `~/.config/com.netuser.desktop/names.json`.
- **Block** copies the device MAC and opens your router's admin page. Blocking happens on the router. The app never interferes with other devices.

## Build from source

Requires Rust, the [Tauri v2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux) and `cargo install tauri-cli --version '^2'`.

```sh
cd src-tauri
cargo test
cargo tauri build   # packages land in src-tauri/target/release/bundle/
```
