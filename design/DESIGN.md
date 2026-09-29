# Kagami — Design Aesthetics

Visual language for the Kagami project and its tools (`feed`, `ledger`, `lens`). This document governs anything a person looks at: the debug page, the lookup page, the frontend, the README badges, the docs site. It also governs what a machine reads (§11): Kagami exists to counter disinformation campaigns that are increasingly run by bots, and its answer is to make sourced, provenance-carrying data as cheap for an agent to consume as it is for a person.

---

## 1. Principles

1. **A mirror, not a lens.** The interface reflects the data without editorializing. No colour-coding of outlets by "side," no emotive iconography, no alarm-red for disinformation. The one accent colour marks *interaction and emphasis*, never *judgment*.
2. **Modern, sharp, quiet.** White space does the work. Hairline rules instead of boxes. One accent, used sparingly, so that when it appears it means something.
3. **Serif for identity, sans for work.** The name and headings carry a serif with character. Everything a person reads for information is a neutral sans. Data and code are monospace. Three families, no more.
4. **Optimal over decorative.** Every visual choice should survive the question "does this make the data easier to read?" If not, cut it.
5. **Fast.** Fonts are self-hosted, subset, and `font-display: swap`. Total font payload under 200 KB. No web-font for body text on the debug page — system sans is fine there.

---

## 2. Colour

### Primary accent

| Token | Value | Use |
|---|---|---|
| `--kg-blue` | `rgb(37, 99, 235)` / `#2563EB` | Links, primary buttons, active states, focus rings, selected items, key data highlights |
| `--kg-blue-hover` | `#1D4ED8` | Hover on the above |
| `--kg-blue-soft` | `#DBEAFE` | Selected-row backgrounds, subtle highlight fills, chart area fills at 30–40% opacity |
| `--kg-blue-ink` | `#1E3A8A` | Blue text on `--kg-blue-soft` when contrast is needed |

`#2563EB` on white passes AA at normal text size (contrast ≈ 5.2:1). Use `--kg-blue-ink` for small text on the soft tint.

### Neutrals

Cool-leaning greys, so they sit naturally next to the blue.

| Token | Value | Use |
|---|---|---|
| `--kg-bg` | `#FFFFFF` | Page background. Pure white, always. |
| `--kg-bg-subtle` | `#F8FAFC` | Alternate rows, code blocks, sidebars |
| `--kg-line` | `#E2E8F0` | Hairline rules, table borders, dividers |
| `--kg-line-strong` | `#CBD5E1` | Input borders, emphasised dividers |
| `--kg-text-muted` | `#64748B` | Captions, metadata, timestamps, secondary labels |
| `--kg-text-secondary` | `#334155` | Body text in dense contexts, table cells |
| `--kg-text` | `#0F172A` | Primary text, headings. Near-black, not pure black. |

### Semantic (use rarely)

| Token | Value | Use |
|---|---|---|
| `--kg-ok` | `#059669` | Feed healthy, checks passing |
| `--kg-warn` | `#D97706` | Backoff, stale, degraded |
| `--kg-err` | `#DC2626` | Fetch failure, parse error |

Semantic colours are for **system status only** — never for characterizing content, outlets, or claims. A story is never red.

### Dark mode

Deferred. The white background is part of the identity for v1. When added: `--kg-bg` `#0B1120`, text `#E2E8F0`, blue lifts to `#3B82F6` for contrast. Not before pillar 3.

---

## 3. Typography

Three families, all open source (SIL OFL), all self-hosted. All available on Google Fonts for prototyping.

### Display / wordmark — **Instrument Serif**

High-contrast, slightly condensed, sharp terminals. Reads as editorial and contemporary rather than bookish. Excellent at large sizes; the italic is especially good and is the wordmark's default style.

- Wordmark: `Kagami` set in Instrument Serif **Italic**, tracking −0.01em, `--kg-text`. Optionally followed by the kanji `鏡` in Noto Serif JP at 60% of the Latin cap height, `--kg-text-muted`.
- Page titles and H1: Instrument Serif Regular, 40–56px, line-height 1.05, tracking −0.02em.
- H2: Instrument Serif Regular, 28–32px, line-height 1.15.
- Never below 24px. It is a display face; at small sizes it loses its edge.

Fallback: `Instrument Serif, "Iowan Old Style", "Times New Roman", serif`.

Alternates considered: Fraunces (more personality, too warm), Newsreader (better for long text, less distinctive as a wordmark), Cormorant (too delicate at UI weights).

### Interface / body — **Inter**

The default choice for a reason: enormous glyph coverage, tabular figures, optical sizing, and it disappears behind the content. Use the variable font.

- Body: 15px / 1.6, `--kg-text-secondary` in dense layouts, `--kg-text` in articles.
- UI labels, buttons, nav: 13px (`ui`), weight 500, tracking 0.
- Small metadata: 12px, weight 400, `--kg-text-muted`, tracking 0.01em.
- H3 and below: Inter Semibold 600, 16–20px. Below H2 the serif hands off to the sans.
- Enable `font-feature-settings: "tnum", "cv11", "ss01"` for tabular numbers and the single-storey a in UI contexts.

Fallback: `Inter, -apple-system, "Segoe UI", system-ui, sans-serif`.

Alternate: Geist Sans if a slightly more geometric feel is wanted later. Do not mix the two.

### Data / code — **JetBrains Mono**

Clear at small sizes, distinct zero, good italics. Used for: cluster IDs, hashes, timestamps in tables, API responses, config samples, terminal output.

- 13px / 1.5, weight 400. Never bold in running data; use colour or weight 500 for emphasis.

Fallback: `"JetBrains Mono", "SF Mono", Menlo, Consolas, monospace`.

### Japanese

For the `鏡` glyph and any Japanese UI: **Noto Serif JP** paired with Instrument Serif, **Noto Sans JP** paired with Inter. Load the JP families only on pages that need them; they are large.

### Type scale

Based on 15px, ratio ~1.25. Eight steps, and nothing else: a size that is
not in this table is a bug. Every step has a fixed line height that is a
multiple of 4px, so baselines land on the spacing grid and stacked text
never produces fractional pixels. Line heights are tokens
(`--kg-lh-*`); never inherit body's line height into a heading, and never
write a unitless ratio.

| Token | Size / line | Family, weight | Tracking | Use |
|---|---|---|---|---|
| `small` | 12 / 16 | Inter 400 | 0.01em | Metadata, provenance line, captions, table headers (500, uppercase, 0.06em) |
| `ui` | 13 / 20 | Inter 400–500, or JetBrains Mono 400 | 0 | UI labels, notes, scope lines, mono data, timeline times |
| `body` | 15 / 24 | Inter 400 | 0 | Running text, list rows, story-list titles |
| `body-lg` | 17 / 24 | Inter 600 | −0.005em | Cluster card title, claim value |
| `h3` | 20 / 28 | Inter 600 | −0.01em | Section heads below H2, outlet names on a story |
| `h2` | 28 / 32 | Instrument Serif 400 | −0.01em | Section heads |
| `h1` | 40 / 44 | Instrument Serif 400 | −0.02em | Page title, one per page |
| `display` | 56 / 60 | Instrument Serif 400 | −0.02em | Home masthead only |

Rules:
- **One H1 per page.** The page title, in the serif. Story pages use the
  story's primary title at `h1`; if it runs past three lines at 720px it
  steps down to `h2` size, never smaller.
- **Weight carries hierarchy, not size, below 20px.** Between 12 and 17
  the steps are small; use 600 against 400 to separate a title from its
  metadata rather than adding a size.
- **Muted is always small.** `--kg-text-muted` is only ever set at 12 or
  13px. Muted body text fails contrast intent; use `--kg-text-secondary`.
- **Numbers.** Tabular figures everywhere (`tnum` is on for the whole
  page). Counts in prose stay Inter; ids, hashes, clock times and raw
  values are JetBrains Mono at `ui`.
- **Measure.** Running text is capped at 68ch inside the 720px column.
  Headlines may use the full column.
- **Row alignment.** The only time a size takes another step's line height
  is to share a baseline with its neighbour in the same row: a 13px mono
  time beside a 15px headline sits on the headline's 24px line, and the
  kanji sits on the lockup's line.
- **Truncation.** Headlines are never truncated or ellipsised: they are
  the outlet's words. Long URLs and hashes may break anywhere
  (`overflow-wrap: anywhere`).

---

## 4. Spacing, grid, shape

- Base unit 4px. Spacing tokens: 4, 8, 12, 16, 24, 32, 48, 64, 96 (`--kg-s1` … `--kg-s9`). Every margin, padding and gap uses one of them. The single exception is the inline tag (below), whose 2px vertical padding keeps it inside a 16px line.
- Content column max 720px for reading; data tables and cluster views may go to 1200px.

**Where each step goes.** Spacing is chosen by relationship, not by eye:

| Step | px | Between |
|---|---|---|
| `s1` | 4 | A line and its own qualifier: title → metadata, value → scope, claim → provenance |
| `s2` | 8 | Items inside one row: meta items, tag and text, icon and label; list-row vertical padding |
| `s3` | 12 | Column gutter inside a row (time column → content); claim card vertical padding |
| `s4` | 16 | Page side gutter on phones; cluster card vertical padding; header bottom padding |
| `s5` | 24 | H3 → its content; between groups inside a section; empty-state padding |
| `s6` | 32 | Above an H2; page top padding on desktop |
| `s7` | 48 | Between major sections of a long page |
| `s8` | 64 | Above the footer |
| `s9` | 96 | Page bottom padding |

- Space above a heading is always larger than space below it (H2: `s6` above, `s2` below; H3: `s5` above, `s2` below), so a heading belongs to what follows.
- Rows in a list are separated by a hairline and `s2` padding each side; cards by a hairline and `s4`. No margins between rows, only padding, so rules stay evenly spaced.
- Prose: paragraphs and lists `0 0 s4`; list indent `s5`; list items `s1` apart.

**Breakpoints.** Two, content-driven: at `max-width: 720px` the side gutter drops to `s4`, H1 to 28/32 and `display` to 40/44; two-column rows (timeline, story list counts) stack. Nothing else changes; the same page, narrower.

**Touch.** Anything clickable on its own (not inline in a sentence) has at least a 32px hit area, from padding, not from a larger font.
- Corner radius: **4px** on inputs and buttons, **6px** on cards, **0** on tables and rules. Nothing rounder — pill shapes read as consumer app, not instrument.
- Borders are 1px `--kg-line`. No drop shadows except a single `0 1px 2px rgba(15,23,42,0.06)` on floating elements (menus, popovers).
- Rules over boxes: separate sections with a hairline, not a bordered container.

---

## 5. Components

**Buttons**
- Primary: `--kg-blue` fill, white text, Inter 500 13px on a 20px line, 4px 16px padding, 32px minimum height, radius 4. Hover `--kg-blue-hover`.
- Secondary: white fill, 1px `--kg-line-strong` border, `--kg-text`. Hover: border `--kg-text-muted`.
- Ghost: no border, `--kg-blue` text. For tertiary actions only.
- Never more than one primary button visible in a view.

**Links**

Blue marks the thing a person is most likely to act on (§1.2), so most
links are not blue. Four kinds, chosen by where the link sits, and every
link is exactly one of them:

| Kind | Class | At rest | Hover / focus | Where |
|---|---|---|---|---|
| Title | `.kg-link-title` | inherits the heading's colour (`--kg-text`), no underline | `--kg-blue`, underline | Cluster card titles, story-list headlines, outlet names used as headings. The object's name is the link. |
| Action | `a` (default) | `--kg-blue`, no underline | `--kg-blue-hover`, underline | The one next step in a view: "all 12 stories", "open the timeline", pagination, search results. Few per screen. |
| Quiet | `.kg-link-quiet` | inherits (`--kg-text-secondary` or muted), underline in `--kg-line-strong` | `--kg-blue`, underline in blue | Links inside metadata and provenance lines: source names, outlet links in a meta row, footer, discussion numbers. |
| Prose | `.kg-prose a` | `--kg-blue`, underline in `--kg-line-strong` | underline in blue | Links inside running text, where the underline is how you find them. |

- Underlines are 1px, `text-underline-offset: 3px`, `text-decoration-skip-ink: auto`. Colour transitions over `--kg-t-fast`; nothing else animates.
- **No visited style.** The mirror doesn't track where a reader has been.
- **Focus** is the global 2px `--kg-blue` outline, offset 2px, on every kind. Never remove it.
- **Leaving the site.** Links to an outlet's article or a source's page open in the same tab, carry `rel="noopener"`, and add no icon; the provenance line already says it is the source.
- A link's text says where it goes ("BBC", "Wikidata", "12 stories"). Never "here", never a bare URL except in a mono tag.

**Tables**
- Header row: Inter 500 12px uppercase, tracking 0.06em, `--kg-text-muted`, bottom rule `--kg-line-strong`.
- Rows: 1px `--kg-line` separators, no zebra striping by default; `--kg-bg-subtle` hover. 40px row height.
- Numbers right-aligned, tabular, mono for IDs and hashes.
- Selected row: `--kg-blue-soft` background, 2px `--kg-blue` left edge.

**Cluster card** (the core object across all three tools)
- Primary title in Inter 600 17px.
- Metadata line: outlet count · item count · velocity · age, Inter 12px muted, separated by `·`.
- Spread sparkline, per the sparkline rules below.
- Origin outlet shown as a small mono tag. No colour by outlet.

**Sparklines**

One chart type, one job: how many outlets had carried a story at each
step since it was first seen. Spec:

- **Size** 80×20 in a list, 160×32 on the story page. The SVG is drawn at that size (no `preserveAspectRatio="none"` stretching), and strokes use `vector-effect: non-scaling-stroke`.
- **Data** cumulative distinct outlets, twelve two-hour buckets over the first 24 hours. Cumulative means the line never falls; a flat line means nobody new picked it up.
- **Scale** the y axis is shared across every sparkline on a page (max = the largest story on the page), and starts at zero. Never scale each line to its own maximum: that draws a 2-outlet story and a 40-outlet story as the same shape, which is exactly the editorialising the mirror avoids.
- **Marks** 1.5px `--kg-blue` line, round joins; area below in `--kg-blue-soft` at 40%; a 1px `--kg-line` baseline; a 3px `--kg-blue` dot on the last point. No axes, gridlines, labels or hover tooltips.
- **Label** the final count sits to the right of the chart as `small` tabular text ("12"), so the number is read, not estimated.
- **Accessibility** `role="img"` with an `aria-label` giving every value ("Outlets carrying the story, two-hour steps: 1, 1, 3, 5 …"). No fact lives only in the line (§11).
- **Single-outlet stories** draw only the baseline and the end dot. An empty curve is still information.

**Charts**
- Line and area only for spread curves; `--kg-blue` stroke 1.5px, `--kg-blue-soft` fill at 40%.
- Axes `--kg-line`, labels Inter 12px (`small`) muted, set with a class, never a `font-size` attribute. No gridlines except a baseline. The chart has an `aria-label` giving its numbers.
- When several series must be distinguished, use blue plus greys of different weight. Introduce a second hue only when unavoidable, and never a red/blue pairing.

**Inputs**
- 1px `--kg-line-strong`, radius 4, 36px height, Inter 13px. Focus: 2px `--kg-blue` ring, no border colour change.

**Status dots**
- 8px circle, semantic colour, always paired with a text label. Never colour alone.

**Claim card** (the core object of `ledger`, and of any surface that shows a fact about an outlet)
- Value in Inter 600 17px. The value is the source's wording, not ours.
- Scope line (where the source's row covers more or less than the outlet) in Inter 13px `--kg-text-secondary`.
- Provenance line beneath, always. A value without its provenance line is a bug.
- A note, if the source or importer has one, in 13px `--kg-text-secondary`, prefixed "Note:".
- Claims from different sources for the same field sit one above the other, same weight, in source order. Never merged, never one shown as the answer.

**Provenance line** (under every fact on every surface, one form everywhere)
- Inter 12px muted, items separated by `·`: source name as a link to the page that states it · licence in a mono tag · "retrieved YYYY-MM-DD" · "as of YYYY-MM-DD" when the source says · status dot with "N days old" or "stale, N days old".
- Order is fixed so a reader learns it once. Nothing is dropped when space is short; the line wraps.
- For observed sources (feed behaviour, site checks) the line also states the window and counts and ends with "a measurement, not a verdict".

---

**Label bar** (coverage by a named source's labels, the honest form of a left/centre/right bar)
- One bar per source and field, headed "<source> on <field>" with the source's licence tag. It is that source's map of the outlets in the story, never Kagami's.
- Segments in the source's own scale order (AllSides: Left, Lean Left, Center, Lean Right, Right; Wikipedia's perennial list: Generally reliable, No consensus, Generally unreliable, Deprecated, Blacklisted), width proportional to the number of outlets, the label text and count on every segment. Greys of different weight, darkest first; never a hue per side.
- Outlets the source does not label are the last segment, drawn empty and dashed, "not rated by <source>". They are never dropped, and they are never distributed across the others.
- Shown only when the source labels at least two of the outlets in the story; a bar with one labelled outlet is a point, not a distribution.
- A legend beneath lists the outlets under each label, by name, alphabetically.

## 6. Wordmark & mark

- Wordmark: `Kagami` in Instrument Serif Italic, `--kg-text`. Minimum height 20px.
- Mark (for favicon, avatars): a hairline circle, 1.5px `--kg-text`, bisected by a single vertical line offset 10% left of centre — a mirror's edge, in profile. Blue variant for active/selected contexts. Keep it geometric; no reflections, no gradients, no eyes.
- Clear space: the cap height of the wordmark on all sides.
- Sub-tool lockups: `Kagami` in serif, then a thin vertical rule, then the tool name in Inter 500 lowercase: **Kagami** │ feed.
- Lockup sizes are fixed, not scaled from one number: wordmark 28px (`h2`), kanji 17px, tool name 13px (`ui`) at weight 500, `s3` either side of the rule. The lockup is the same size on every screen.

---

## 7. Motion

- Durations: 120ms for hover/focus, 200ms for enter/exit, `cubic-bezier(0.2, 0, 0, 1)`.
- Only opacity and transform. Nothing bounces, nothing pulses.
- Live-updating views (debug page, cluster lists) fade new rows in over 200ms; they never slide or push.

---

## 8. Voice in the interface

- Sentence case everywhere, including buttons and headings. No title case, no shouting.
- Label things by what they are: "12 outlets", "first seen 14:02 UTC", "3 syndication groups". Never "trending", "viral", "breaking".
- Timestamps in UTC by default with local time on hover.
- Empty states are one short sentence and, if relevant, one action. No illustrations.

---

## 9. Do / don't

**Do**
- Let white space separate things.
- Use blue for the one thing on screen the person is most likely to act on.
- Show numbers in tabular mono.
- Keep the serif large and rare.

**Don't**
- Colour-code outlets, stories, or claims by political lean or credibility.
- Use red for anything but system errors.
- Add shadows, gradients, glassmorphism, or rounded pills.
- Use more than three type families or more than one accent hue.
- Set the serif below 24px or use it in tables.

---

## 10. One language across surfaces

Every page any tool serves is a view of the same mirror, so they share:

- **Chrome.** The lockup top left (`Kagami` │ tool), one health or count line top right, a hairline under the header. No navigation bar; each tool is one page deep and links to the others' pages by URL.
- **Objects.** The cluster card and the claim card are the two objects. A story is always a cluster card; a fact is always a claim card with its provenance line. `lens` composes them; it does not invent a third.
- **Outlets.** Shown by name with the domain muted beside it, never with a colour, badge or icon that carries a rating. The origin outlet of a story gets the **origin ring**, and that is the only distinction:
  - a 1.5px `--kg-text` ring, 1.5px clear of the edge, around the outlet's icon (`.kg-origin`); where a surface shows no icons, the same ring around the outlet's name (`.kg-origin-name`);
  - near-black, not blue: blue means "act here", and the ring is a fact, not an action;
  - the word "first" is not written beside it. The ring carries "first to publish" as its accessible name (visually hidden text, `.kg-sr`), and the JSON and Markdown twins keep the word, since no fact lives only in a mark (§11).
- **Time.** UTC by default, local on hover or by a control in the chrome, same control on every page. Ages as "N days old", "6 in the last hour", never relative words like "recently".
- **Absence.** Empty and not-found states are one sentence naming what would fill them ("No claims yet. Run `ledger import wikidata`."). The same sentence shape everywhere.
- **Copy, don't share.** Each tool carries its own copy of the tokens and component CSS, verified against `preview.html`. A change to a component is made here first, then copied.

## 11. Machine readers

Agents and bots are first-class readers, not scrapers to tolerate. Disinformation is spread by automation; the counter is sourced data that automation can pick up with provenance attached, so that a bot quoting Kagami carries the source, licence and date along whether it means to or not.

- **Every page has a data twin.** Any HTML page is a rendering of a JSON route, linked from its `<head>` with `<link rel="alternate" type="application/json">`. Nothing is shown to a person that a machine cannot get from the API.
- **Negotiate, don't guess.** Data routes answer `Accept: text/markdown` with a Markdown rendering that keeps every provenance line, so a language model reading the page gets the same facts in the same order as a person. JSON remains the default.
- **Announce yourself.** Every server serves `/llms.txt` describing what it is, the routes, the contract version and the rules of use, `/v1/openapi.json` describing every route and object so a tool-using agent can call it without guessing, and `/robots.txt` that allows everything. The rules of use are the project's: quote the source, not Kagami; carry the licence; never turn claims into a score.
- **Stable addresses.** Outlet ids, item ids and cluster ids are the contract; a URL that worked keeps working. Ids are also in the page as `data-id` so an agent driving a browser can read them without parsing prose.
- **Plain structure.** Semantic HTML (`article`, `header`, `time`), text labels for every status, `aria-label` on charts giving the numbers. No fact lives only in colour, an icon or a sparkline.
- **Provenance is not optional in any format.** Markdown, JSON, HTML and export all carry source, licence, retrieved date and ref URL on each claim. A format that drops them is not a Kagami format.

## 12. Implementation notes

- Tokens live in one `tokens.css` shared across tools; each tool copies it, no shared runtime dependency (project rule: bespoke, no framework).
- Fonts: `design/fonts/fetch.sh` downloads variable Inter, static Instrument Serif (Regular + Italic) and variable JetBrains Mono from a pinned google/fonts commit, subsets them to Latin + Latin Extended with `pyftsubset`, and writes `woff2` (about 164 KB together, under the 200 KB budget). `lens build` copies them into the site's `fonts/`. Noto JP loaded on demand.
- Conformance: `lens` has a test that its copy of `tokens.css` is byte-identical to this directory's, and one that its stylesheets use only scale sizes and spacing tokens. The other surfaces follow the same rules by hand.
- The `#2563EB` blue coincides with a well-known utility framework's `blue-600`. That's fine — it's a good blue — but no tool in the project uses that framework; tokens are hand-written.
