#!/bin/bash
set -euo pipefail

# Build only the user-facing CLI.
cargo build --release -p rusty-claude-cli

# Install the Rust migration under the rouraTUI product name. Keep the
# imported kernel's historical command as a compatibility alias.
mkdir -p "$HOME/.local/bin"
install -m 0755 "target/release/claw" "$HOME/.local/bin/rouratui"
ln -sf "rouratui" "$HOME/.local/bin/claw"

echo "✓ rouraTUI installed to ~/.local/bin/rouratui"
