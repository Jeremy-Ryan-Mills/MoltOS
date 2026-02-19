//! Scheduler implementations.

pub mod round_robin;
pub mod eevdf;

pub use eevdf::EevdfScheduler as Scheduler;
