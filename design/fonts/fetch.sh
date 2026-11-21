#!/usr/bin/env sh
# Download, subset and write the three First Report font families as woff2
# into this directory. Needs curl and fonttools with brotli:
#
#   pip install fonttools brotli
#   sh design/fonts/fetch.sh
#
# Sources are pinned to one google/fonts commit so the output is
# reproducible. All three families are SIL OFL 1.1.
set -eu

REV=23e54b51ddffbc7713c583748e3bd86f62b1fa4a
BASE="https://raw.githubusercontent.com/google/fonts/$REV/ofl"
HERE=$(cd "$(dirname "$0")" && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Latin, Latin Extended-A/B, general punctuation, arrows, minus, euro, trademark.
UNICODES="U+0000-00FF,U+0100-024F,U+2000-206F,U+2190-2193,U+2212,U+20AC,U+2122"

# Ubuntu's python3-fonttools has the module but not the pyftsubset command.
if command -v pyftsubset >/dev/null; then PYFTSUBSET=pyftsubset; else PYFTSUBSET="python3 -m fontTools.subset"; fi

subset() { # <in.ttf> <out.woff2> <features>
  $PYFTSUBSET "$1" --unicodes="$UNICODES" --layout-features="$3" \
    --flavor=woff2 --no-hinting --desubroutinize --output-file="$2"
  echo "  $(basename "$2"): $(wc -c < "$2" | tr -d ' ') bytes"
}

fetch() { # <url> <out>
  curl -fsSL "$1" -o "$2"
}

echo "downloading from google/fonts@$REV"
fetch "$BASE/instrumentserif/InstrumentSerif-Regular.ttf" "$TMP/is-r.ttf"
fetch "$BASE/instrumentserif/InstrumentSerif-Italic.ttf" "$TMP/is-i.ttf"
fetch "$BASE/inter/Inter%5Bopsz,wght%5D.ttf" "$TMP/inter.ttf"
fetch "$BASE/jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf" "$TMP/jbm.ttf"

echo "subsetting"
subset "$TMP/is-r.ttf"  "$HERE/InstrumentSerif-Regular.woff2" "kern,liga"
subset "$TMP/is-i.ttf"  "$HERE/InstrumentSerif-Italic.woff2"  "kern,liga"
subset "$TMP/inter.ttf" "$HERE/Inter-Variable.woff2"          "kern,liga,calt,tnum,cv11,ss01"
subset "$TMP/jbm.ttf"   "$HERE/JetBrainsMono-Variable.woff2"  "kern,zero"

total=$(cat "$HERE"/*.woff2 | wc -c | tr -d ' ')
echo "total: $total bytes (target under 200000)"
