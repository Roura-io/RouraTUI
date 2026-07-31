#!/bin/zsh

set -euo pipefail

if (( $# != 1 )); then
  print -u2 -- "Usage: scripts/release-m3.zsh <version>"
  exit 2
fi

readonly VERSION="$1"
readonly TAG="v${VERSION}"
readonly REPOSITORY="Roura-io/RouraTUI"
readonly UNAS_HOST="root@10.0.2.2"
readonly UNAS_ROOT="/volume/ad445dbf-95a1-4e31-9ea1-25e8112be804/RouraIO/artifacts"
readonly PROJECT_ROOT="$(git rev-parse --show-toplevel)"
readonly GH_BIN="${HOME}/.local/bin/gh"

cd "$PROJECT_ROOT"

[[ "$(git branch --show-current)" == "main" ]] || {
  print -u2 -- "Releases must be built from main."
  exit 1
}
[[ -z "$(git status --porcelain)" ]] || {
  print -u2 -- "The main working tree must be clean."
  exit 1
}
grep -Fq "version = \"${VERSION}\"" Cargo.toml || {
  print -u2 -- "Cargo.toml does not declare version ${VERSION}."
  exit 1
}
git rev-parse --verify --quiet "refs/tags/${TAG}" >/dev/null && {
  print -u2 -- "Tag ${TAG} already exists."
  exit 1
}

cargo fmt --all -- --check
cargo clippy -p rouratui-cli --all-targets -- -D warnings
cargo test -p rouratui-cli input::tests --quiet
cargo test -p rouratui-cli --test compact_repl_panic --quiet
cargo build --locked --release -p rouratui-cli

readonly STAGING_ROOT="$(mktemp -d /tmp/rouratui-release.XXXXXX)"
trap 'rm -rf -- "$STAGING_ROOT"' EXIT

cp target/release/rouratui "$STAGING_ROOT/rouratui"
tar -czf "$STAGING_ROOT/rouratui-darwin-arm64.tar.gz" -C "$STAGING_ROOT" rouratui
(
  cd "$STAGING_ROOT"
  shasum -a 256 rouratui-darwin-arm64.tar.gz > rouratui-darwin-arm64.tar.gz.sha256
)

git tag -a "$TAG" -m "rouraTUI ${VERSION}"
git push origin "$TAG"
"$GH_BIN" release create "$TAG" \
  "$STAGING_ROOT/rouratui-darwin-arm64.tar.gz" \
  "$STAGING_ROOT/rouratui-darwin-arm64.tar.gz.sha256" \
  --repo "$REPOSITORY" \
  --title "rouraTUI ${TAG}" \
  --notes-file RELEASE-NOTES.md

ssh "$UNAS_HOST" "mkdir -p '${UNAS_ROOT}/exports/RouraTUI/${VERSION}' '${UNAS_ROOT}/manifests/RouraTUI/${VERSION}'"
scp "$STAGING_ROOT/rouratui-darwin-arm64.tar.gz" "${UNAS_HOST}:${UNAS_ROOT}/exports/RouraTUI/${VERSION}/"
scp "$STAGING_ROOT/rouratui-darwin-arm64.tar.gz.sha256" "${UNAS_HOST}:${UNAS_ROOT}/manifests/RouraTUI/${VERSION}/"

print -- "Published ${TAG} from the M3 and archived its artifacts on the UNAS."
