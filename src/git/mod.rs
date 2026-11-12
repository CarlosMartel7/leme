//! Read-only access to Git.

mod discovery;
mod worktrees;

pub use discovery::{Checkout, DiscoveryError, discover};
pub use worktrees::{InvalidBranch, Worktree, check_branch_name, checkout_of, worktrees};
