#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/cache"
DIST="$ROOT/dist"
OUT="$ROOT/out"
KOBO="${1:-$OUT/dicthtml-et-ru.zip}"

if [[ ! -s "$KOBO" ]]; then
  echo "Kobo dictionary not found: $KOBO" >&2
  echo "Build it first with ./scripts/build.sh" >&2
  exit 1
fi

mkdir -p "$CACHE/bin" "$DIST" "$OUT"

KINDLING="$ROOT/cache/bin/kindling-cli"
if [[ ! -x "$KINDLING" ]]; then
  if command -v kindling-cli >/dev/null 2>&1; then
    KINDLING="$(command -v kindling-cli)"
  else
    echo "Downloading kindling-cli..."
    case "$(uname -s)-$(uname -m)" in
      Darwin-arm64|Darwin-aarch64) kurl="https://github.com/ciscoriordan/kindling/releases/download/v0.38.0/kindling-cli-mac-apple-silicon" ;;
      Darwin-x86_64|Darwin-amd64) kurl="https://github.com/ciscoriordan/kindling/releases/download/v0.38.0/kindling-cli-mac-intel" ;;
      Linux-x86_64|Linux-amd64) kurl="https://github.com/ciscoriordan/kindling/releases/download/v0.38.0/kindling-cli-linux" ;;
      *)
        echo "Install kindling-cli (https://github.com/ciscoriordan/kindling) and re-run" >&2
        exit 1
        ;;
    esac
    curl -sL --fail -o "$KINDLING" "$kurl"
    chmod +x "$KINDLING"
  fi
fi

echo "Building Kindle source from $KOBO ..."
cargo build --release --manifest-path "$ROOT/Cargo.toml" --bin build-kindle
"$ROOT/target/release/build-kindle" --kobo "$KOBO" --outdir "$DIST/kindle"

echo "Generating Kindle dictionary..."
"$KINDLING" build "$DIST/kindle/dict.opf" -o "$OUT/dict-et-ru.mobi"
rm -f "$OUT/dict-et-ru.mobi.zip"
zip -9 -j "$OUT/dict-et-ru.mobi.zip" "$OUT/dict-et-ru.mobi"
echo "Wrote $OUT/dict-et-ru.mobi.zip"
