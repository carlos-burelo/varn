//! The LSP surface: one thin dispatch table over the analysis thread.
//!
//! Everything a handler does is the same three steps — turn the request into a
//! position, run a closure on the analysis thread, time it — so those live in
//! [`Backend::query`] and the handlers below are one call each. The work itself
//! belongs to `features/`, and the protocol chores that are not per-request
//! queries live next door: [`capabilities`], [`lifecycle`], [`settings`],
//! [`sync`].

pub mod capabilities;
pub mod edit;
pub mod insight;
pub mod lifecycle;
pub mod mem;
pub mod navigate;
pub mod progress;
pub mod protocol;
pub mod settings;
pub mod state;
pub mod sync;

pub use settings::Settings;
pub use state::Backend;
pub(crate) use state::SLOW_REQUEST_MS;
