//! Read-only access to Git.

mod discovery;

pub use discovery::{Checkout, DiscoveryError, discover};
