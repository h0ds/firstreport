# reply — Voice

How the bot sounds. Every template in `templates/` is held to this, and
a pull request that changes a sentence is reviewed against it before
the golden diffs are read. Rule 6 says what a reply may contain; this
says how it is said.

## 1. Who is talking

A records clerk who has read everything and takes no side. Not a
fact-checker, not a debunker, not a referee, not a friend. The clerk
was summoned, looked things up, and is reading back what the record
says, in the order it happened. The clerk finds the record interesting
and assumes you will too. The clerk does not care who wins.

People will @ the bot to prove a point. Both sides get the same clerk,
the same sentences, the same links. The clerk's only loyalty is to the
record being complete and easy to check.

## 2. What it sounds like

- **Plain past tense.** Things were published, registered, copied,
  reached. "First published by X at 09:00 UTC." Not "appears to have",
  not "seems to originate".
- **Specific.** A time, a count, a name, a date. Never "recently",
  "many", "a fringe site", "a major outlet".
- **Short sentences, one fact each.** Every sentence has one cite, so
  every sentence has one fact. If a sentence needs two cites it is two
  sentences.
- **The reader's inference is the reader's.** "Its domain was registered
  19 days ago" ends there. Not "which is suspicious", not "so be careful",
  not a wink.
- **A miss is stated as a miss.** "Kagami has no story for this.
  Checked: X, Y." Not "couldn't find anything", not "sorry", not
  "that doesn't mean it's false".
- **Says its own name.** "Kagami has it from 3 outlets." Kagami is the
  system whose record this is, so its limits are named as its limits:
  "not an outlet Kagami follows."
- **Explains a term once, in the sentence that uses it.** "That is one
  copy, not independent reporting" earns its place after a syndication
  number. Nothing else is explained.
- **Same voice on a miss and a hit.** The clerk is not more animated
  when the record is damning and not apologetic when it is empty.

## 3. What it never says

- **Verdicts.** True, false, fake, real, misleading, debunked,
  confirmed, legitimate, unreliable, propaganda, disinformation. Not
  even hedged. "Kagami does not rate stories" is also not said; the
  absence is the statement.
- **Labels for people or accounts.** Bot, troll, sockpuppet, astroturf,
  coordinated, state-backed, shill. An observation is a count and a
  window: "41 accounts posted this link within 6 minutes."
- **Adjectives about outlets.** Fringe, mainstream, reputable, obscure,
  partisan, right-wing, left-wing, tabloid. An outlet has a name, an
  owner of record, a founding date, a domain registration date, and the
  ratings that named third parties gave it, each attributed to its
  rater.
- **Address to the poster.** No "you", no "your post", no "OP". The
  reply is about the story, and the person who summoned the bot is not
  in it.
- **Advice.** "Check before sharing", "consider the source", "be
  skeptical". The clerk reads the record; what to do with it is not the
  clerk's business.
- **Emotion, humour, sarcasm, emoji, exclamation marks.** A reply that
  can be screenshotted as a dunk has failed, even when the record is
  one-sided.
- **Hedges that pretend to be modesty.** "Just", "might", "possibly",
  "it seems". If the record says it, say it. If the record does not
  say it, do not say it.
- **Filler.** "Here's what I found", "great question", "interesting",
  "let me look". The first sentence is a fact.
- **Anything without a cite.** Enforced by `check`, but the reason is
  voice too: a sentence nobody can click through to verify is an
  opinion.

## 4. Tests for a new sentence

Before a sentence becomes a template, it passes all of these:

1. **Both sides.** Read it as the person who wanted the story to be
   true, then as the person who wanted it false. It must read the same
   to both: informative, not vindicating.
2. **Screenshot.** Cropped to this one sentence with the bot's name on
   it, does it still say only what the record says? A sentence that
   becomes a slogan out of context is rewritten.
3. **Cite.** Point at the URL. Does that page state exactly this, in
   these units, for this window? If the sentence rounds, the sentence
   says "about". If the sentence summarises, the summary is a fact the
   page shows.
4. **Empty case.** What does the reply look like when this template is
   the only one that fires? It must still be a complete, honest reply.
5. **Delete test.** Remove the sentence. Is the reply less informative?
   If it is not, the sentence is not added.
6. **Read aloud.** In a flat voice, at a desk. Any word that wants a
   raised eyebrow to land is cut.

## 5. Shape of a reply

1. The oldest fact first: origin. Then this link's place in it. Then
   spread. Then syndication. Then the outlet. Then observations, which
   are reviewed by a human before they post. Then the closing line with
   the full record.
2. A miss replaces the story sentences with what was checked.
3. The closing line is always present and always the same. It is the
   one sentence that talks about the bot rather than the story:
   "Every fact above links its source; full record: …"
4. One reply per mention. A second summons on the same post gets the
   same record's link. The clerk does not argue.

## 6. Names and numbers

- Outlets by the name in `feed`'s registry, never the domain, unless
  the domain is the fact ("truth-wire.example is not an outlet Kagami
  follows").
- Times in UTC with the zone written: "09:00 UTC". Dates as
  `YYYY-MM-DD`. Durations as "1 h 30 min", "45 s", "3 days".
- Counts as digits. Shares as whole percentages. Never "most", "nearly
  all", "a majority".
- Sources by the name `ledger` gives them: "per Wikidata", "per the
  domain registry (RDAP)". Never "according to reports".
- Kagami's own measurements are attributed to Kagami: "Kagami has it
  from 3 outlets", "Kagami first saw it at".

## 7. Languages

The English templates set the voice; a translation is held to this
document, not to the English wording. A translated sentence that needs
a hedge or an honorific the English lacks adds it; the tests in §4
still apply.
