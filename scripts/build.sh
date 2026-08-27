#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/cache"
DIST="$ROOT/dist"
OUT="$ROOT/out"
EVS_URL="https://arhiiv.eki.ee/litsents/idkaart/evs/evs_EKI_CCBY40.xml"

mkdir -p "$CACHE" "$DIST" "$OUT" "$ROOT/cache/bin"

EVS="$CACHE/evs_EKI_CCBY40.xml"
if [[ ! -s "$EVS" ]]; then
  echo "Downloading EVS XML..."
  curl -L --fail --retry 3 -o "$EVS" "$EVS_URL"
fi

DICTGEN="$ROOT/cache/bin/dictgen"
if [[ ! -x "$DICTGEN" ]]; then
  if command -v go >/dev/null 2>&1; then
    echo "Installing dictgen with Go..."
    GOBIN="$ROOT/cache/bin" go install github.com/pgaskin/dictutil/cmd/dictgen@v0.3.2
  else
    echo "Downloading dictgen binary..."
    case "$(uname -s)-$(uname -m)" in
      Darwin-arm64|Darwin-aarch64)
        echo "Apple Silicon needs Go to build dictgen (no arm64 release binary)." >&2
        exit 1
        ;;
      Darwin-x86_64|Darwin-amd64) url="https://github.com/pgaskin/dictutil/releases/download/v0.3.2/dictgen-darwin-64bit" ;;
      Linux-arm*|Linux-aarch64) url="https://github.com/pgaskin/dictutil/releases/download/v0.3.2/dictgen-linux-arm" ;;
      Linux-*) url="https://github.com/pgaskin/dictutil/releases/download/v0.3.2/dictgen-linux-64bit" ;;
      *) echo "Install Go and re-run, or place dictgen at $DICTGEN" >&2; exit 1 ;;
    esac
    curl -sL --fail -o "$DICTGEN" "$url"
    chmod +x "$DICTGEN"
  fi
fi

echo "Building dictfile..."
cargo build --release --manifest-path "$ROOT/Cargo.toml" --bin build-est-ru-df
"$ROOT/target/release/build-est-ru-df" \
  --evs "$EVS" \
  --inflections "$ROOT/data/est_inflected_forms.tsv" \
  --output "$DIST/est-ru.df"

echo "Generating Kobo dictionary..."
"$DICTGEN" -o "$OUT/dicthtml-et-ru.zip" "$DIST/est-ru.df"
echo "Wrote $OUT/dicthtml-et-ru.zip"
