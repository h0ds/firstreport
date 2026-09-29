//! Platform adapters (plan §5). A listener yields mentions; a poster
//! puts an approved draft under the mention it answers, as a thread when
//! it is more than one post.

pub mod bluesky;
pub mod x;

use std::future::Future;
use std::pin::Pin;

use crate::model::Mention;

pub trait Listener {
    fn name(&self) -> &str;
    /// Mentions since the last poll, oldest first. Idempotence is the
    /// caller's job: the queue's seen set drops repeats.
    fn poll(&mut self) -> impl Future<Output = Result<Vec<Mention>, String>> + Send;
}

/// What posting returns: the ids made so far, and the error if it stopped.
pub type Posted = Result<Vec<String>, (Vec<String>, String)>;

/// Object-safe so the server can hold whichever platform is configured.
pub trait Poster: Send {
    fn name(&self) -> &str;
    /// Post `posts` in reply to `m`, each after the previous; returns the
    /// platform ids of the posts made. A failure part-way returns what
    /// was posted in the error so the log can record it.
    fn post<'a>(
        &'a mut self,
        m: &'a Mention,
        posts: &'a [String],
    ) -> Pin<Box<dyn Future<Output = Posted> + Send + 'a>>;
}
