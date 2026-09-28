//! Getting bytes off the network: a due-time scheduler with backoff and an
//! HTTP client that does conditional requests under concurrency limits.

pub mod client;
pub mod scheduler;
