#!/usr/bin/env python3
"""Turn a directory of the aggregator scrape JSON files into evaluation data.

Source: a public MIT-licensed scraper of a news aggregator; its json/
directory holds 72 stories scraped in Aug 2025.
The raw scrape is NOT in this repository. Run this once against a local
checkout of it:

    python3 extract.py /path/to/aggregator-scrape/json

Outputs, next to this script:

  aggregator-pairs.tsv    label<TAB>text_a<TAB>text_b for `feed eval`.
                          Positives are two sources a news aggregator grouped
                          under the same story; negatives come from
                          different stories, weighted toward pairs with
                          high lexical overlap (the hard negatives that
                          decide the threshold).
  aggregator-domains.txt  distinct article hosts with counts, for seeding
                          outlets.toml via `feed discover`. No bias labels
                          are carried over: those are the aggregator's
                          aggregation and are not ours to ship.

Only outlet headlines, short snippets and article hosts are used. Ground
News's own perspective summaries are editorial text and are left alone.
Deterministic: same input, same output.
"""

import glob
import json
import os
import random
import re
import sys
from collections import Counter, defaultdict
from urllib.parse import urlsplit

SNIPPET_CHARS = 240
POS_PER_STORY = 6
NEG_HARD_PER_STORY = 8
NEG_RANDOM_PER_STORY = 4
SEED = 7

STOP = set("a an the and or but of to in on at for with by from as is are was were be been it its this that these those he she they his her their we you i not no has have had will would can could says said after over into than who what how why when where".split())


def norm_tokens(s):
    return [w for w in re.findall(r"[a-z0-9]+", s.lower()) if w not in STOP]


def clean(s):
    return re.sub(r"\s+", " ", (s or "")).strip()


def text_of(row):
    title = clean(row.get("news_title"))
    snippet = clean(row.get("text"))[:SNIPPET_CHARS]
    if snippet and not snippet.endswith((".", "!", "?")):
        snippet = snippet.rsplit(" ", 1)[0] + "…" if " " in snippet else snippet
    return f"{title} . {snippet}" if snippet else title


def host_of(url):
    try:
        h = urlsplit(url).hostname or ""
    except ValueError:
        return ""
    h = h.lower()
    for p in ("www.", "amp.", "m.", "mobile."):
        if h.startswith(p):
            h = h[len(p):]
    return h


def load(json_dir):
    stories = []
    for path in sorted(glob.glob(os.path.join(json_dir, "*.json"))):
        try:
            d = json.load(open(path, encoding="utf-8"))
        except (OSError, ValueError):
            continue
        rows = []
        seen_titles = set()
        for r in d.get("sources") or []:
            if not isinstance(r, dict):
                continue
            title = clean(r.get("news_title"))
            if len(title) < 15:
                continue
            key = title.lower()
            if key in seen_titles:
                continue  # identical syndicated copies add nothing to the pair set
            seen_titles.add(key)
            rows.append({"text": text_of(r), "host": host_of(r.get("news_link") or "")})
        if len(rows) >= 2:
            stories.append({"id": d.get("story_id") or os.path.basename(path), "title": clean((d.get("metadata") or {}).get("title")), "rows": rows})
    # The scrape repeats some stories under new ids. Two stories sharing
    # half their headlines are one story; keep the first.
    kept = []
    for s in stories:
        titles = {r["text"].lower() for r in s["rows"]}
        dup = any(len(titles & {r["text"].lower() for r in k["rows"]}) * 2 >= min(len(titles), len(k["rows"])) for k in kept)
        if not dup:
            kept.append(s)
    return kept


def jaccard(a, b):
    a, b = set(a), set(b)
    return len(a & b) / len(a | b) if a | b else 0.0


def build_pairs(stories):
    rng = random.Random(SEED)
    pairs = []
    toks = {}
    for si, s in enumerate(stories):
        for ri, r in enumerate(s["rows"]):
            toks[(si, ri)] = norm_tokens(r["text"])

    # Positives: sampled pairs within a story.
    for si, s in enumerate(stories):
        idx = list(range(len(s["rows"])))
        cands = [(i, j) for i in idx for j in idx if i < j]
        rng.shuffle(cands)
        for i, j in cands[:POS_PER_STORY]:
            pairs.append((1, s["rows"][i]["text"], s["rows"][j]["text"]))

    # Hard negatives: for each story, the cross-story rows that overlap
    # most lexically with one of its rows.
    all_keys = list(toks.keys())
    for si, s in enumerate(stories):
        scored = []
        for ri in range(len(s["rows"])):
            a = toks[(si, ri)]
            for (sj, rj) in all_keys:
                if sj == si:
                    continue
                scored.append((jaccard(a, toks[(sj, rj)]), ri, sj, rj))
        scored.sort(reverse=True)
        used = set()
        n = 0
        for _, ri, sj, rj in scored:
            if sj in used:
                continue
            used.add(sj)
            pairs.append((0, s["rows"][ri]["text"], stories[sj]["rows"][rj]["text"]))
            n += 1
            if n >= NEG_HARD_PER_STORY:
                break

    # Random negatives, as an easy floor.
    for si, s in enumerate(stories):
        for _ in range(NEG_RANDOM_PER_STORY):
            sj = rng.randrange(len(stories))
            if sj == si:
                continue
            a = rng.choice(s["rows"])["text"]
            b = rng.choice(stories[sj]["rows"])["text"]
            pairs.append((0, a, b))

    # Deduplicate and order deterministically.
    seen = set()
    out = []
    for lab, a, b in pairs:
        if a == b:
            continue
        k = (lab, min(a, b), max(a, b))
        if k in seen:
            continue
        seen.add(k)
        out.append(k)
    out.sort()
    return out


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)
    here = os.path.dirname(os.path.abspath(__file__))
    stories = load(sys.argv[1])
    pairs = build_pairs(stories)
    pos = sum(1 for p in pairs if p[0] == 1)
    with open(os.path.join(here, "aggregator-pairs.tsv"), "w", encoding="utf-8") as f:
        f.write("# Derived from the aggregator scrape's json/ directory by extract.py; see README.md.\n")
        f.write(f"# {len(stories)} stories, {sum(len(s['rows']) for s in stories)} distinct headlines, {pos} positive and {len(pairs) - pos} negative pairs.\n")
        for lab, a, b in pairs:
            f.write(f"{lab}\t{a}\t{b}\n")

    hosts = Counter()
    per_story = defaultdict(set)
    for s in stories:
        for r in s["rows"]:
            if r["host"]:
                hosts[r["host"]] += 1
                per_story[r["host"]].add(s["id"])
    with open(os.path.join(here, "aggregator-domains.txt"), "w", encoding="utf-8") as f:
        f.write("# host\tarticles\tstories  (from the aggregator scrape json/; no ratings carried over)\n")
        for h, n in sorted(hosts.items(), key=lambda kv: (-kv[1], kv[0])):
            f.write(f"{h}\t{n}\t{len(per_story[h])}\n")

    print(f"{len(stories)} stories, {sum(len(s['rows']) for s in stories)} headlines, {pos} positives, {len(pairs) - pos} negatives, {len(hosts)} hosts")


if __name__ == "__main__":
    main()
