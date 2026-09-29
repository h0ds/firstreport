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

/// What to search the platform for when observing a subject.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchQuery {
    Url(String),
    Text(String),
}

impl SearchQuery {
    /// The observation subjects this query maps to.
    pub fn subjects(&self) -> Vec<String> {
        match self {
            SearchQuery::Url(u) => vec![u.clone()],
            SearchQuery::Text(t) => {
                vec![format!("text:{:016x}", crate::observe::simhash::simhash(t))]
            }
        }
    }
}

/// One post found by a search, before hashing. The account id never
/// leaves the process unhashed.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub platform: String,
    pub account_id: String,
    pub created_at: Option<i64>,
    pub at: i64,
    pub url: Option<String>,
    pub text_hash: Option<u64>,
}

/// Object-safe: search recent posts carrying a subject.
pub trait Searcher: Send {
    fn name(&self) -> &str;
    fn search<'a>(
        &'a mut self,
        q: &'a SearchQuery,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Found>, String>> + Send + 'a>>;
}

/// What posting returns: the ids made so far, and the error if it stopped.
pub type Posted = Result<Vec<String>, (Vec<String>, String)>;

/// Object-safe so the server can hold whichever platform is configured.
pub trait Poster: Send {
    fn name(&self) -> &str;
    /// Post `posts` in reply to `m`, each after the previous, in `lang`
    /// (ISO 639-1, for platforms that tag posts); returns the platform
    /// ids of the posts made. A failure part-way returns what was posted
    /// in the error so the log can record it.
    fn post<'a>(
        &'a mut self,
        m: &'a Mention,
        posts: &'a [String],
        lang: &'a str,
    ) -> Pin<Box<dyn Future<Output = Posted> + Send + 'a>>;
}
