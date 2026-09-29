//! Turning fetched bytes into `Item`s: feed parsing, text cleanup, URL
//! canonicalization, outlet resolution, and the simhash syndication signal.

pub mod feed;
pub mod html;
pub mod outlet;
pub mod revision;
pub mod simhash;
pub mod telegram;
pub mod url;
