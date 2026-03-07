# Vesa

[![CI](https://github.com/jbj338033/vesa/actions/workflows/ci.yml/badge.svg)](https://github.com/jbj338033/vesa/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A cross-platform software KVM built in Rust. Share your keyboard and mouse across multiple computers over the network.

## Features

- **Zero-config encryption** — QUIC transport with self-signed certificates (trust-on-first-use)
- **Low-latency input** — Datagrams for mouse/keyboard events, reliable streams for control messages
- **Cross-platform** — macOS (CGEventTap), Windows (Low-Level Hooks), Linux (evdev)
- **Menu-bar app** — System tray GUI (Tauri + Svelte), no Dock icon
- **Lightweight protocol** — Custom binary format, max 21 bytes per message

## Quick Start

```bash
pnpm install
pnpm tauri dev
```

### Build

```bash
pnpm tauri build               # App bundle (.app / .msi / .AppImage)
```

## How It Works

```
Server Machine                              Client Machine
┌──────────────┐    QUIC datagram    ┌──────────────┐
│  Input       │    (mouse/key)      │  Input       │
│  Capture     │ ──────────────────> │  Emulation   │
│              │    QUIC stream      │              │
│  (CGEventTap │ <────────────────>  │  (CGEvent    │
│   / LL Hook  │    (enter/leave)    │   / SendInput│
│   / evdev)   │                     │   / XTest)   │
└──────────────┘                     └──────────────┘
```

1. Move your cursor to the screen edge configured for a client
2. Vesa captures all keyboard and mouse input on the server
3. Input events are sent as QUIC datagrams (loss-tolerant, low-latency)
4. The client injects the events using native OS APIs
5. Press **Escape** (configurable) to release capture

## Configuration

Vesa uses a TOML config file (`~/.config/vesa/config.toml`):

```toml
# Server mode
[server]
bind_addr = "0.0.0.0:4920"
release_hotkey = "Escape"

[[server.clients]]
name = "laptop"
position = "Right"

[[server.clients]]
name = "desktop"
position = "Left"
```

```toml
# Client mode
[client]
server_addr = "192.168.1.100:4920"
position = "Right"
```

## Architecture

```
crates/
├── vesa-event     # Shared types (InputEvent, Position, ScreenBounds)
├── vesa-proto     # Binary protocol encode/decode
├── vesa-capture   # Input capture (macOS/Windows/Linux)
├── vesa-emulate   # Input injection (macOS/Windows/Linux)
├── vesa-net       # QUIC transport (Quinn) + self-signed certs
└── vesa-core      # Server/client orchestration + config
src-tauri/         # Tauri v2 backend (menu-bar app)
src/               # Svelte 5 frontend
```

Dependencies flow left-to-right: `event` ← `proto` ← `net` ← `core` ← `tauri`.

## Platform Support

| Platform | Capture | Emulation | Status |
|----------|---------|-----------|--------|
| macOS    | CGEventTap | CGEvent post HID | Implemented |
| Windows  | SetWindowsHookExW | SendInput | Stub |
| Linux    | evdev grab | XTest | Stub |

> macOS requires **Accessibility** permission. Vesa will prompt automatically on first launch.

## Testing

```bash
cargo test               # Run all 55 tests
cargo clippy             # Zero warnings
```

| Crate | Tests | Coverage |
|-------|-------|----------|
| vesa-event | 12 | Type conversions, serde roundtrips |
| vesa-proto | 15 | Protocol encode/decode, error cases |
| vesa-net | 8 | QUIC loopback, certs, multi-connection |
| vesa-core | 19 | Config parsing, defaults, positions, edge detection |

## Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Rust (Edition 2024) |
| Runtime | Tokio |
| Transport | QUIC (Quinn 0.11) |
| TLS | rustls 0.23 |
| Certificates | rcgen 0.13 |
| macOS Input | core-graphics 0.25 |
| GUI | Tauri v2 + Svelte 5 |
| Config | TOML |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

[MIT](LICENSE)
