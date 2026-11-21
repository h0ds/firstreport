# design — the visual layer

Everything a person looks at across First Report's tools lives here as the source of
truth. Tools copy `tokens.css` into their own tree (project rule: bespoke, no
shared runtime dependency) and are checked against `preview.html`.

- `DESIGN.md` — the aesthetics document: principles, colour, type, spacing, components, voice.
- `tokens.css` — every token from `DESIGN.md` as CSS custom properties plus base component classes (`kg-*`).
- `preview.html` — a static page exercising every component. Open it in a browser; no build step.
- `mark.svg` — the mark: a hairline circle bisected by a line offset 10% left of centre.
- `fonts/` — self-hosted font bundle (gitignored) and fetch/subset instructions.

Dark mode is deferred to pillar 3.
