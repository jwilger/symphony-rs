#![forbid(unsafe_code)]
#![deny(warnings)]
#![deny(
    clippy::all,
    clippy::cargo,
    clippy::complexity,
    clippy::correctness,
    clippy::perf,
    clippy::style,
    clippy::suspicious,
    clippy::multiple_crate_versions
)]

mod service;
mod ui;

#[cfg(feature = "hydrate")]
mod hydrate;

pub use service::run;
