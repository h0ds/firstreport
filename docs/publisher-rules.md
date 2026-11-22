# What First Report takes from publishers, and the rules it follows

First Report reads what publishers put out for machines to read: feeds and
news sitemaps. It keeps headline, link, summary when the feed carries
one, and time. It never fetches article pages, never stores article
text, and never republishes anything but a headline with a link back.
This page says exactly what is fetched and which rules govern it, so a
publisher can check us against their own terms.

## What is fetched

| What | From | Kept |
|---|---|---|
| RSS, Atom and JSON feeds | the URL the publisher advertises | headline, link, summary (at most 512 characters), published time, language |
| Google News sitemaps | the sitemap URL, for publishers with no feed | headline, link, published time, language; nothing else in the entry |
| robots.txt | every origin, once a day | the rules, applied before any other request |
| The home page | once a month, by `ledger import site` | whether HTTPS is served and which accountability pages the home page links |

Raw feed and sitemap bodies are archived compressed as evidence of what
was published when; they are the publisher's own machine-readable
output, not article pages. Retention is configurable and defaults to
180 days.

## Rules the fetcher enforces

These are code, not policy; the tests in `feed/src/fetch/` check them.

- **The opt-out list comes first.** A host in `feed/config/opt-out.toml`
  is never requested at all: not its feeds, not its robots.txt, not its
  home page, not by discovery, and the same for its subdomains. Every
  entry carries the reason and the date, and entries are only added by
  hand.
- **robots.txt is honoured** (RFC 9309). The group addressed to
  `firstreport-feed` wins, else `*`. A disallowed path is never requested;
  a feed under a disallowed path fails with that reason in `/v1/feeds`
  and is retried only when the rule changes, checked daily.
- **Crawl-delay is honoured**, capped at two minutes, as the spacing
  between any two requests to the host.
- **We identify ourselves.** The User-Agent is
  `firstreport-feed/<version> (+https://github.com/h0ds/firstreport)`. Hosts that
  reject anything not browser-shaped get the `Mozilla/5.0 (compatible;
  firstreport-feed/…)` form, which still names the project and links here.
- **Conditional requests.** ETag and Last-Modified are sent; a 304 costs
  the publisher nothing. Intervals adapt: an unchanged feed is polled
  less often, and a feed that changes at most every fifteen minutes by
  default, never below `poll.min_interval_secs`.
- **Rate limits are respected.** 429 and 503 back the feed off
  exponentially and honour `Retry-After`. At most two requests to a
  host at a time, sixty-four in flight overall.
- **Budgets, even when nothing asks for them.** At most 240 requests to
  a host per hour and 20,000 overall by default; a request over budget
  waits, and the waits are counted in `/v1/stats`.
- **Permanent moves are followed and remembered**, so a publisher who
  moves a feed is not polled at the old address.
- **No article fetching.** Nothing under a feed or sitemap entry's link
  is requested by `feed`. The one page request First Report makes to a
  publisher is the monthly home-page check, and it too honours
  robots.txt.

## What we ask of ourselves

- A publisher who wants out edits their robots.txt, which we read
  daily, or opens an issue on the repository, which is the contact
  address in every User-Agent. Either is honoured without argument; an
  issue becomes a line in `feed/config/opt-out.toml`, which is checked
  before anything else.
- Terms of use that restrict automated access are read in that light:
  a feed exists to be polled and a sitemap to be crawled, and First Report
  requests nothing else. Where a publisher's terms say otherwise for
  those files, the entry is dropped. This is the project's reading, not
  legal advice.
- Attribution on every surface: a headline is always shown with the
  outlet's name and a link to the outlet's own page; lens never shows
  article text because it never has any.

## Entries that need a word

- **Reuters** publishes no feed. Its Google News sitemap is fetched as
  reuters.com/robots.txt allows. Reuters' terms restrict automated use
  of its content; the sitemap is the file it publishes for exactly this
  kind of reading, and First Report keeps headline, link and time from it and
  nothing more.
- **Associated Press** blocks feed readers on apnews.com; the entry is
  a third-party mirror and is labelled as such in the config.
