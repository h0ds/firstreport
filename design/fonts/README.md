# Fonts

Self-hosted, subset, `font-display: swap`. Files here are gitignored; run
`./fetch.sh` to populate. Total payload target: under 200 KB.

| Family | Files | Source |
|---|---|---|
| Instrument Serif | `InstrumentSerif-Regular.woff2`, `InstrumentSerif-Italic.woff2` | github.com/Instrument/instrument-serif (OFL) |
| Inter | `Inter-Variable.woff2` | github.com/rsms/inter (OFL) |
| JetBrains Mono | `JetBrainsMono-Variable.woff2` | github.com/JetBrains/JetBrainsMono (OFL) |

Subsetting: Latin + Latin Extended with `pyftsubset`, e.g.

```
pyftsubset Inter-Variable.ttf --unicodes="U+0000-00FF,U+0100-024F,U+2000-206F,U+20AC,U+2122" \
  --layout-features="kern,liga,tnum,cv11,ss01" --flavor=woff2 --output-file=Inter-Variable.woff2
```

Noto Serif JP / Noto Sans JP are loaded on demand only on pages that need
the `鏡` glyph or Japanese UI; they are not part of this bundle.

The debug page in `feed` does not load a web font for body text; system sans
is fine there.
