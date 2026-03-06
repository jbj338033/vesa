# Contributing to Vesa

Thank you for your interest in contributing to Vesa! This document provides guidelines to make the contribution process smooth for everyone.

## Development Environment

### Prerequisites

- Rust stable (Edition 2024)
- Node.js 18+
- pnpm
- Platform-specific requirements:
  - **macOS**: Xcode Command Line Tools, Accessibility permission for input capture
  - **Windows**: Visual Studio Build Tools
  - **Linux**: `libevdev-dev`, `libxtst-dev`

### Setup

```bash
git clone https://github.com/jbj338033/vesa.git
cd vesa
cargo build
pnpm install
```

### Running

```bash
cargo test            # Run all tests
cargo clippy          # Lint check
cargo fmt --check     # Format check
pnpm tauri dev        # GUI development
```

## Coding Conventions

- **Zero warnings, zero clippy lints** — CI enforces `cargo clippy --all-targets -- -D warnings`
- **No unnecessary `as` casts** — use proper types from official documentation
- **Edition 2024** — use latest Rust idioms
- **Platform code** — use `cfg(target_os)` with separate module files
- Run `cargo fmt` before committing

## Pull Request Process

1. Fork the repository and create a feature branch from `main`
2. Make your changes with clear, focused commits
3. Ensure all checks pass:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```
4. Open a pull request with a clear description of the changes
5. Link any related issues

## Commit Messages

Use clear, descriptive commit messages:

```
feat: add Linux evdev input capture backend
fix: handle QUIC connection timeout gracefully
docs: update configuration examples
refactor: simplify protocol encoding logic
test: add roundtrip tests for mouse events
```

Prefix with `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, or `chore:` as appropriate.

## Reporting Issues

When reporting a bug, please include:

- Operating system and version
- Vesa version (`vesa-cli --version` or check `Cargo.toml`)
- Steps to reproduce
- Expected behavior
- Actual behavior
- Relevant logs (run with `RUST_LOG=debug` for verbose output)

## Feature Requests

Open an issue with the `enhancement` label describing:

- The problem you're trying to solve
- Your proposed solution
- Any alternatives you've considered

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
