# claims/

One `<outlet_id>.toml` per outlet, where the id comes from `feed`'s
`config/outlets.toml`. Each file holds `[[claim]]` entries; every claim
names a source from `../sources.toml`, links the page that states it,
and records when it was retrieved.

Populate with `ledger import wikidata --outlets ../feed/config/outlets.toml --write`
on a machine with network access, then `ledger check`. Hand-written
claims are welcome and survive re-import as long as they don't carry a
`source_id` that the importer owns.
