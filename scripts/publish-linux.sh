#!/usr/bin/env bash
# Publish the Linux x64 build: one binary (FFDec is embedded and unpacked into Tools/
# on first launch; on Linux it runs through `java -jar`) plus the default config.ini.
# Output: publish/linux-x64/habbo-downloader
# Needs Rust (rustup) and the usual GUI build libraries (e.g. libxkbcommon-dev, libgtk-3-dev
# for the folder picker).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT="$ROOT/publish/linux-x64"

cd "$ROOT"
cargo build --release -p habbo-app

rm -rf "$OUT"
mkdir -p "$OUT"
cp target/release/habbo-downloader "$OUT/"
cp crates/core/assets/default_config.ini "$OUT/config.ini"
chmod +x "$OUT/habbo-downloader"

# A .desktop entry next to the binary so file managers can launch it.
cat > "$OUT/all-in-1-converter.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=All-in-1 Converter
Comment=Habbo asset workstation (Professional window / CLI)
Exec=sh -c 'cd "\$(dirname "\$1")" && exec ./habbo-downloader' sh %k
Terminal=true
Categories=Development;Utility;
EOF

echo
echo "Published Linux build at: $OUT"
echo "Run from terminal:  $OUT/habbo-downloader                 (asks Professional or CLI)"
echo "Run from terminal:  $OUT/habbo-downloader --professional"
echo "Run from terminal:  $OUT/habbo-downloader --cli"
