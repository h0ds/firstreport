//! Platform adapters (plan §5). A listener yields mentions; posting is
//! R3 and does not exist yet.

pub mod bluesky;

use std::future::Future;

use crate::model::Mention;

pub trait Listener {
    fn name(&self) -> &str;
    /// Mentions since the last poll, oldest first. Idempotence is the
    /// caller's job: the queue's seen set drops repeats.
    fn poll(&mut self) -> impl Future<Output = Result<Vec<Mention>, String>> + Send;
}
