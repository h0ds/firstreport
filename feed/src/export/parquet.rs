//! Parquet export. Enabled with `--features parquet`.

use std::path::Path;

use super::Summary;
use crate::store::reader::Reader;

pub fn write(
    _reader: &Reader,
    _out_dir: &Path,
    _now: i64,
    _hamming: u32,
) -> Result<Summary, Box<dyn std::error::Error>> {
    Err("parquet export not implemented yet".into())
}
