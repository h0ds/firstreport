//! `/llms.txt` and `/robots.txt` for the static site (design §11).

use crate::input::Input;

pub const ROBOTS_TXT: &str = "User-agent: *\nAllow: /\nSitemap: /sitemap.xml\n";

pub fn llms_txt(input: &Input) -> String {
    let sources: Vec<String> = input
        .sources
        .values()
        .map(|s| format!("- {} ({}): {}", s.name, s.licence, s.url))
        .collect();
    format!(
        "# First Report lens

> Stories forming across news outlets, with the record of who published first,
> who joined when, and what was copied, and sourced facts about each outlet.
> A static site built from First Report feed and First Report ledger exports. Every number is
> a count from feed; every label is a source's, with its licence and date. Nothing
> here rates an outlet, and there is no score.

Built from an export at {export}. Pages regenerate nightly.

## Pages

- `/` - stories in the window as cluster cards ordered by reach (independent reports plus languages, a count over coverage stated on each card and under `order` in the JSON twin; see /about/#reach), single reports listed apart, then every outlet
- `/story/<id>/` - one story: timeline in join order with headlines side by side, copies, changes, facts about each outlet present
- `/outlet/<id>/` - every claim about one outlet with its provenance line, then its stories and counts over 30 days
- `/search/` - outlets and stories; its `index.json` is the index
- `/about/` - what each number means, what the site does not do, the sources with their licences
- `/sitemap.xml` - every page

Every page has two twins beside it with the same facts in the same order:
`index.json` and `index.md`. Fetch `<page>index.md` for Markdown.

## Rules of use

- Quote the source, not First Report: a claim is what its source says, and its link is the page that says it.
- Carry the licence: each source's terms are on every claim.
- Sources that disagree are listed side by side. Do not average, rank or score them.
- A story's membership is by text similarity, not by a person; treat it as evidence.
- `first` means the earliest published member as feeds reported it; feeds lie about times, and picked-up times are what First Report saw.
- Copies are the same text at more than one outlet, not independent coverage.
- Observed claims (domain registry, site checks, feed behaviour) are measurements with their window and counts attached; they describe what an outlet does, not whether that is good.

## Sources

{sources}
",
        export = crate::render::datetime(input.export_time),
        sources = sources.join("\n")
    )
}
