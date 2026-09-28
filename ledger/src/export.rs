//! NDJSON export: `claims.ndjson`, `sources.ndjson`, `outlets.ndjson`.

use std::io::{BufWriter, Write};
use std::path::Path;

use serde::Serialize;

use crate::store::Ledger;

#[derive(Serialize)]
struct OutletRow<'a> {
    id: &'a str,
    claims: usize,
}

fn lines<T: Serialize>(path: &Path, rows: impl Iterator<Item = T>) -> std::io::Result<usize> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    let mut n = 0;
    for r in rows {
        serde_json::to_writer(&mut w, &r)?;
        w.write_all(b"\n")?;
        n += 1;
    }
    w.flush()?;
    Ok(n)
}

pub fn write(ledger: &Ledger, out_dir: &Path) -> std::io::Result<(usize, usize, usize)> {
    std::fs::create_dir_all(out_dir)?;
    let claims = lines(
        &out_dir.join("claims.ndjson"),
        ledger.claims.values().flatten(),
    )?;
    let sources = lines(&out_dir.join("sources.ndjson"), ledger.sources.iter())?;
    let outlets = lines(
        &out_dir.join("outlets.ndjson"),
        ledger.claims.iter().map(|(id, c)| OutletRow {
            id,
            claims: c.len(),
        }),
    )?;
    Ok((claims, sources, outlets))
}
