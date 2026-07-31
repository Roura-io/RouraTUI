#!/bin/bash
set -euo pipefail

# Build only the user-facing CLI.
cargo build --release -p rouratui-cli

# Install the Rust migration under the rouraTUI product name with its
# local-Ollama launcher. Keep the imported kernel's historical command as a
# compatibility alias.
prefix="${ROURATUI_PREFIX:-$HOME/.local}"
mkdir -p "$prefix/bin" "$prefix/libexec/rouratui"
install -m 0755 "target/release/rouratui" "$prefix/libexec/rouratui/rouratui-bin"
install -m 0755 "scripts/rouratui" "$prefix/bin/rouratui"
ln -sf "rouratui" "$prefix/bin/claw"

echo "✓ rouraTUI installed to $prefix/bin/rouratui"
