//! Shared local engine. Desktop and terminal clients use the same daemon.
pub mod client;
pub mod daemon;
mod metrics;
mod paths;
mod store;

mod studio;
mod scheduler;
pub mod registry;
pub mod connections;

pub(crate) mod changes;
