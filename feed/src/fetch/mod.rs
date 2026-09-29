//! Getting bytes off the network: a due-time scheduler with backoff and
//! adaptive intervals, an HTTP client that does conditional requests under
//! concurrency limits, and the tooling that grows the feed list (OPML
//! import, autodiscovery).

pub mod client;
#[cfg(test)]
mod client_test;
pub mod discover;
pub mod opml;
pub mod opt_out;
pub mod robots;
pub mod scheduler;
