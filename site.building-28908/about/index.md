# About

Kagami tracks how news stories spread: who published first, who followed, what was copied and what was changed later. It also collects facts about each outlet, with sources.

## What the numbers mean

- **Story**: Headlines that feed grouped as the same event because their text is similar. The grouping is automatic and sometimes wrong.
- **First**: The headline with the earliest publish time in its feed. Feeds sometimes get times wrong, so we also show when we picked it up.
- **Outlets, items**: How many outlets covered a story, and how many headlines they published. No outlet counts for more than another.
- **Reach**: How the front page is ordered. It counts outlets that wrote their own version of the story, adds one for each extra language, then breaks ties by recent coverage and by time. Stories only one outlet covered are listed separately. The topic and the outlets involved play no part. Full rules: docs/algorithms.md.
- **Copy**: The same text published by more than one outlet. Copies don't count as separate coverage. Wire stories are meant to be copied.
- **Revised, withdrawn**: The headline or summary changed after publication, or the item disappeared from its feed.
- **Claim**: A fact about an outlet from one source, with a link, the licence and the date we read it. When sources disagree we show both.
- **Stale**: Older than the source's refresh interval, so it may be out of date.
- **Measurement**: Facts we measured ourselves from domain records, the outlet's home page and its feed. Each one gives the time window and counts it's based on.

## What Kagami doesn't do

- Kagami has no score or rating of its own. Lean colours come from AllSides, and the State, Sanctioned and Deprecated tags each come from a named source, shown on hover.
- It doesn't summarise stories. Headlines are the outlets' own.
- It only reads feeds, never full article pages. The raw fetches are kept.
- It doesn't filter outlets by quality. Every feed in the pool is treated the same.

## Sources

- **AllSides Media Bias Ratings** (rating) · licence CC-BY-NC-4.0 · https://www.allsides.com/media-bias/ratings · AllSides' rating of each outlet from Left to Right, using their own method. Non-commercial use only. Attribution: "AllSides Media Bias Ratings™ by AllSides.com are licensed under a Creative Commons Attribution-NonCommercial 4.0 International License." AllSides sends the data on request (admin@allsides.com); import it with `ledger import allsides --file <file>`.
- **EU restrictive measures (Council of the European Union)** (legal) · licence EU legal texts, free reuse (Commission Decision 2011/833/EU) · https://eur-lex.europa.eu/eli/reg/2014/833/oj · Outlets the EU has banned from broadcasting under Regulation 833/2014, as part of sanctions against Russian state propaganda. Each claim links the regulation that named the outlet.
- **Kagami feed behaviour** (observed) · licence Kagami observation · https://github.com/h0ds/kagami/tree/main/feed · How often the outlet posts, how often it edits or removes items, how much it copies and how often it's first. Outlets with fewer than ten items in the window are left out.
- **Domain registries (RDAP)** (observed) · licence public registration data · https://about.rdap.org · When each outlet's domain was registered, the registrar, and the domain's status, from the registry.
- **Partisan audience bias scores (Robertson et al. 2018)** (database) · licence CC0-1.0 · https://dataverse.harvard.edu/dataset.xhtml?persistentId=doi:10.7910/DVN/QAN5VX · Scores from -1 to +1 for about 19,000 websites, based on whether Democrats or Republicans shared their links on Twitter in 2017. This measures the audience, not the content. Robertson, Jiang, Joseph, Friedland, Lazer and Wilson (2018), Auditing Partisan Audience Bias within Google Search.
- **Outlet home page** (observed) · licence Kagami observation · https://github.com/h0ds/kagami/tree/main/ledger · Whether the outlet's site uses HTTPS, and which pages (about, contact, corrections, ethics, staff) its home page links to. A missing page means it isn't linked from the home page, not that it doesn't exist.
- **State Media Monitor (MJRC)** (database) · licence CC-BY-4.0 · https://statemediamonitor.com · Classifies state media by ownership, funding and editorial control: State-Controlled (SC), Captured Public (CaPu), Captured Private (CaPr), and independent public media (IP, ISF, ISM, ISFM). Some entries cover a parent group, such as China Media Group for CGTN. Cite: Dragomir, M. (2025). State Media Monitor Global Dataset 2025. MJRC. DOI: 10.5281/zenodo.17219015.
- **Wikidata** (database) · licence CC0-1.0 · https://www.wikidata.org · Owner, parent company, country, founding date, legal form, website and Wikipedia link. Public domain.
- **Wikipedia: Reliable sources/Perennial sources** (community) · licence CC-BY-SA-4.0 · https://en.wikipedia.org/wiki/Wikipedia:Reliable_sources/Perennial_sources · Wikipedia editors' consensus on how reliable each source is, with their status, summary and links to the discussions. Reuse needs attribution and the same licence.

## For machine readers

Every page has `index.json` and `index.md` beside it. `/llms.txt` states the rules of use; `/sitemap.xml` lists every page; `/search/index.json` is the index. Please cite the original source and keep its licence. Don't turn these facts into a score.

Built from an export at 2026-09-29 23:43 UTC. Source: https://github.com/h0ds/kagami
