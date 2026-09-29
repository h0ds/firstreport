//! One module per page kind. Each returns the three twins for a page:
//! HTML for people, JSON and Markdown for machines, same facts, same
//! order.

pub mod home;
pub mod outlet;
pub mod story;

pub struct Twins {
    pub html: String,
    pub json: serde_json::Value,
    pub md: String,
}
