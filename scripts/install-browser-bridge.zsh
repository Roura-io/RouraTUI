#!/bin/zsh

set -euo pipefail

if (( $# != 1 )); then
  print -u2 -- "Usage: $0 <Chrome extension id>"
  exit 2
fi

readonly EXTENSION_ID="$1"
readonly ROOT="${HOME}/.rouratui"
readonly HOST="${ROOT}/bin/rouratui-browser-host"
readonly EXTENSION="${ROOT}/chrome-extension"
readonly HOST_DIR="${HOME}/Library/Application Support/Google/Chrome/NativeMessagingHosts"
readonly MANIFEST="${HOST_DIR}/com.roura_io.rouratui.json"

[[ -x "$HOST" ]] || { print -u2 -- "Missing browser host: $HOST (run rouratui update first)"; exit 1; }
[[ -d "$EXTENSION" ]] || { print -u2 -- "Missing extension directory: $EXTENSION (run rouratui update first)"; exit 1; }

mkdir -p "$HOST_DIR"
printf '{\n  "name": "com.roura_io.rouratui",\n  "description": "RouraTUI visible Chrome control host",\n  "path": "%s",\n  "type": "stdio",\n  "allowed_origins": ["chrome-extension://%s/"]\n}\n' "$HOST" "$EXTENSION_ID" > "$MANIFEST"

chmod 755 "$HOST"
print -- "Native host registered: $MANIFEST"
print -- "Load unpacked in chrome://extensions from: $EXTENSION"
print -- "Then enable browser tools with: export ROURATUI_ENABLE_BROWSER=1"
