#!/usr/bin/env bash
# Rebuild testdata/corpus and testdata/fonts from pinned, freely licensed sources.
# Every file is public domain, CC0, MIT, Apache-2.0, ISC or OFL (see testdata/corpus/SOURCES.md).
# Results are committed; re-run only to change the selection.
set -euo pipefail
cd "$(dirname "$0")/.."

FLAGS_REV=4612960fd030e9e37648ec0f0cd19a663e27831e    # lipis/flag-icons, MIT
NOTO_REV=99701e07d3d1fe1bdbcdb14964547f0fd592966f     # googlefonts/noto-emoji, images Apache-2.0
LUCIDE_REV=9cb3934c3628dfe3fbb34bc4c12209b2cee3a49f   # lucide-icons/lucide, ISC
FLUENT_REV=1ffb34c752ecf5d402f04cfb4b392c77f57c54bc   # microsoft/fluentui-emoji, MIT
PHOSPHOR_REV=2b75f3ad12b420c9504ef05df8d2564a28f8500e # phosphor-icons/core, MIT
FONTS_REV=5026800714d07231937d92eb4cceabf06e05d74e    # google/fonts, OFL-1.1

FLAGS=(fr jp gb us ca br ch np kr za)
NOTO=(1f600 1f34e 1f680 1f30d 1f525 1f981 1f3e0 2b50 1f437 1f4a1)
LUCIDE=(camera house heart settings bike anchor)
# "Asset folder|file stem|style" — style: flat (aplats), color (dégradés), high_contrast (trait noir & blanc)
FLUENT=("Cat face|cat_face|flat" "Fox|fox|flat" "Hot beverage|hot_beverage|flat" "Bicycle|bicycle|flat"
        "Sun with face|sun_with_face|color" "Butterfly|butterfly|color"
        "Owl|owl|high_contrast" "Guitar|guitar|high_contrast")
PHOSPHOR=("fill|airplane" "fill|coffee" "fill|tree" "duotone|rocket" "duotone|camera")
FONTS=("pacifico|Pacifico-Regular.ttf" "caveat|Caveat[wght].ttf" "bebasneue|BebasNeue-Regular.ttf"
       "abrilfatface|AbrilFatface-Regular.ttf")

C=testdata/corpus
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
get() { curl -fsSL --retry 3 -o "$2" "$1"; }
gh_raw() { printf 'https://raw.githubusercontent.com/%s/%s/%s' "$1" "$2" "$(python3 -I -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$3")"; }

rm -rf "$C/flags" "$C/illustrations" "$C/icons" "$C/emoji" testdata/fonts
mkdir -p "$tmp"/{flags,noto,lucide,fluent,phosphor} testdata/fonts

for f in "${FLAGS[@]}"; do get "$(gh_raw lipis/flag-icons $FLAGS_REV "flags/4x3/$f.svg")" "$tmp/flags/$f.svg"; done
for e in "${NOTO[@]}"; do get "$(gh_raw googlefonts/noto-emoji $NOTO_REV "2D/svg/emoji_u$e.svg")" "$tmp/noto/noto-$e.svg"; done
for i in "${LUCIDE[@]}"; do get "$(gh_raw lucide-icons/lucide $LUCIDE_REV "icons/$i.svg")" "$tmp/lucide/lucide-$i.svg"; done
for entry in "${FLUENT[@]}"; do
  IFS='|' read -r dir stem style <<<"$entry"
  folder=$([[ $style == high_contrast ]] && echo "High Contrast" || echo "${style^}")
  get "$(gh_raw microsoft/fluentui-emoji $FLUENT_REV "assets/$dir/$folder/${stem}_${style}.svg")" "$tmp/fluent/fluent-${stem}-${style//_/-}.svg"
done
for entry in "${PHOSPHOR[@]}"; do
  IFS='|' read -r weight name <<<"$entry"
  get "$(gh_raw phosphor-icons/core $PHOSPHOR_REV "assets/$weight/$name-$weight.svg")" "$tmp/phosphor/phosphor-$name-$weight.svg"
done
for entry in "${FONTS[@]}"; do
  IFS='|' read -r dir file <<<"$entry"
  get "$(gh_raw google/fonts $FONTS_REV "ofl/$dir/$file")" "testdata/fonts/$file"
  get "$(gh_raw google/fonts $FONTS_REV "ofl/$dir/OFL.txt")" "testdata/fonts/OFL-$dir.txt"
done

# Normalize: fixed render size per category, concrete colors instead of currentColor.
N="python3 -I scripts/normalize_svg.py"
$N "$tmp/flags" "$C/flags" 384 288
$N "$tmp/noto" "$C/illustrations" 256 256
$N "$tmp/fluent" "$C/illustrations" 256 256
$N "$tmp/lucide" "$C/icons" 128 128 --stroke '#1f2937' --background '#ffffff'
$N "$tmp/phosphor" "$C/icons" 128 128 --stroke '#1f2937' --background '#ffffff'

# Public-domain / CC0 files from Wikimedia Commons (license checked per file).
python3 -I scripts/fetch_commons.py fetch "$C/commons.tsv" "$C"
# Project-made cases (lettering with the OFL fonts above, pixel art, chart).
python3 -I scripts/gen_corpus.py testdata

echo "Corpus: $(find "$C" -type f \( -name '*.svg' -o -name '*.png' -o -name '*.jpg' \) | wc -l) files"
