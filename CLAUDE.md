# Vesa - Software KVM

Cross-platform software KVM in Rust. Shares keyboard and mouse across multiple computers over the network. Tauri v2 desktop app (no CLI).

## Quick Start

```bash
cargo build              # Build workspace crates
cargo test               # Run all tests (43)
cargo clippy             # Lint
pnpm tauri dev           # Launch Tauri app (requires icon files: pnpm tauri icon)
```

## Architecture

```
crates/
├── vesa-event     # Shared event types (InputEvent, Position, ScreenBounds)
├── vesa-proto     # Binary protocol encode/decode (1B tag + fixed payload, big-endian)
├── vesa-capture   # Input capture trait + platform backends (macOS: CGEventTap)
├── vesa-emulate   # Input injection trait + platform backends (macOS: CGEvent post HID)
├── vesa-net       # QUIC transport (Quinn) + rcgen self-signed certificates
└── vesa-core      # Server/client core logic + TOML config
src-tauri/         # Tauri v2 backend (menu-bar app, tray icon)
src/               # Svelte 5 frontend
```

Dependency direction: `event` ← `proto` ← `net` ← `core` ← `src-tauri`. `capture` and `emulate` depend only on `event`.

## App Behavior

- **Menu-bar only** (`ActivationPolicy::Accessory`): no Dock icon, tray icon only
- Left-click tray icon toggles settings window; right-click shows context menu (Settings / Quit)
- Window close hides instead of quitting
- macOS cursor hide/show: single `CGDisplayHideCursor`/`CGDisplayShowCursor` call in `set_capturing()` (GUI process — no per-frame hack needed)

## Key Conventions

- **Edition 2024**, Rust stable
- Maintain **zero warnings, zero clippy lints**
- QUIC datagram = input events (loss-tolerant, low-latency). QUIC stream = control messages (Enter/Leave/Ack, reliable)
- Max protocol message size is 21 bytes. Well within MTU floor of 1200B
- macOS: uses core-graphics 0.25. `CallbackResult` enum, `mach_port()` method accessor
- Minimize `as` casts. Use proper types from official docs
- Platform-specific code uses `cfg(target_os)` + separate module files. Windows/Linux are stubs for now

## Frontend

- Svelte 5 with `$state`, `$props`, `$effect` runes
- `DisplayArrangement.svelte`: drag-and-snap monitor placement (macOS-style). Props: `{ position, peerLabel, disabled?, onchange }`
- `ClientView.svelte` / `ServerView.svelte`: use `DisplayArrangement` for screen position
- Tauri commands: `start_server(bindAddr, clientPosition)`, `stop_server`, `start_client(serverAddr, position)`, `stop_client`, `get_status`

## Testing

```bash
cargo test -p vesa-event   # 12 tests: type conversion roundtrips, serde
cargo test -p vesa-proto   # 15 tests: protocol roundtrips, error cases, size verification
cargo test -p vesa-net     # 8 tests: QUIC loopback (datagram + stream), certs, multi-connection
cargo test -p vesa-core    # 8 tests: TOML config parsing, defaults, Position variants
```

## Dependencies

| Purpose | Crate | Version |
|---------|-------|---------|
| Runtime | tokio | 1 |
| QUIC | quinn | 0.11 |
| TLS | rustls | 0.23 |
| Certificates | rcgen | 0.13 |
| macOS input | core-graphics | 0.25 |
| Config | serde + toml | 1 / 0.8 |
| GUI | tauri | 2 |
| Frontend | svelte | 5 |
| Package manager | pnpm | - |

## Common Tasks

**Adding a platform backend**: Implement in `vesa-capture/src/{platform}.rs` and `vesa-emulate/src/{platform}.rs`, then add the `cfg` branch in each `lib.rs`.

**Adding a protocol message**: Add tag constant + Message variant + encode/decode branches + tests in `vesa-proto/src/lib.rs`. Avoid tag collisions with existing messages.

**Adding a Tauri command**: Add function in `src-tauri/src/commands.rs`, register in `invoke_handler` in `src-tauri/src/lib.rs`, call from Svelte via `invoke()`.

**Tauri development**: `pnpm tauri dev` (icon files required — generate with `pnpm tauri icon`).
