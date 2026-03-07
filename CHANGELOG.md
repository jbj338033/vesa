# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cross-platform software KVM with QUIC transport
- macOS input capture via CGEventTap
- macOS input injection via CGEvent post HID
- Custom binary protocol (max 21 bytes per message)
- Self-signed certificate generation (trust-on-first-use)
- TOML configuration file support
- Tauri v2 GUI with Svelte 5 frontend
- Menu-bar only app (no Dock icon, tray icon toggle)
- Display Arrangement UI for drag-and-snap monitor placement
- Windows and Linux platform stubs

### Fixed

- Cursor hide/show now uses single API call instead of per-frame hack (GUI process)
