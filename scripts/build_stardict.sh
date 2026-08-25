#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/cache"
DIST="$ROOT/dist"
OUT="$ROOT/out"
KOBO="${1:-$OUT/dicthtml-et-ru.zip}"
OPF="$DIST/kindle/dict.opf"
STARDICT="$OUT/stardict-et-ru"

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

if [[ ! -f "$OPF" ]]; then
  echo "Building dictionary source from $KOBO ..."
  python3 "$ROOT/scripts/build_kindle.py" --kobo "$KOBO" --outdir "$DIST/kindle"
fi

echo "Generating StarDict dictionary..."
"$KINDLING" stardict "$OPF" -o "$STARDICT" \
  --bookname "Eesti-vene sõnaraamat (et-ru)" \
  --author "Eesti Keele Instituut" \
  --description "CC BY-SA 4.0. Derived from EKI Eesti-vene sõnaraamat."
DICT="$STARDICT/stardict-et-ru.dict"
if [[ -f "$DICT" ]]; then
  gzip -9 -n -c "$DICT" > "$DICT.dz"
  rm -f "$DICT"
fi
rm -f "$OUT/stardict-et-ru.zip"
(cd "$OUT" && zip -9 -r stardict-et-ru.zip stardict-et-ru -x '*.dict' -x '*.DS_Store')
echo "Wrote $OUT/stardict-et-ru.zip"
