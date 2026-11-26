#!/usr/bin/env bash
# Download the third-party ground-truth SVGs of the benchmark corpus into testdata/corpus/.
# Sources are pinned to a commit; every file is permissively licensed (see testdata/corpus/SOURCES.md).
# The files are committed, so this only needs re-running to change the selection.
set -euo pipefail
cd "$(dirname "$0")/.."

FLAGS_REV=4612960fd030e9e37648ec0f0cd19a663e27831e   # lipis/flag-icons, MIT
NOTO_REV=99701e07d3d1fe1bdbcdb14964547f0fd592966f    # googlefonts/noto-emoji, images Apache-2.0
LUCIDE_REV=9cb3934c3628dfe3fbb34bc4c12209b2cee3a49f  # lucide-icons/lucide, ISC

FLAGS=(fr jp gb us ca br ch np kr za)
EMOJI=(1f600 1f34e 1f680 1f30d 1f525 1f981 1f3e0 2b50 1f437 1f4a1)
ICONS=(camera house heart settings bike anchor)

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/flags" "$tmp/emoji" "$tmp/icons"

get() { curl -fsSL --retry 3 -o "$2" "$1"; }
for f in "${FLAGS[@]}"; do get "https://raw.githubusercontent.com/lipis/flag-icons/$FLAGS_REV/flags/4x3/$f.svg" "$tmp/flags/$f.svg"; done
for e in "${EMOJI[@]}"; do get "https://raw.githubusercontent.com/googlefonts/noto-emoji/$NOTO_REV/2D/svg/emoji_u$e.svg" "$tmp/emoji/$e.svg"; done
for i in "${ICONS[@]}"; do get "https://raw.githubusercontent.com/lucide-icons/lucide/$LUCIDE_REV/icons/$i.svg" "$tmp/icons/$i.svg"; done

# Normalize: fixed render size per category, concrete stroke color for icons.
python3 -I scripts/normalize_svg.py "$tmp/flags" testdata/corpus/flags 384 288
python3 -I scripts/normalize_svg.py "$tmp/emoji" testdata/corpus/emoji 256 256
python3 -I scripts/normalize_svg.py "$tmp/icons" testdata/corpus/icons 128 128 --stroke '#1f2937' --background '#ffffff'
echo "Corpus updated: $(find testdata/corpus -name '*.svg' | wc -l) SVG files"
