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

#[cfg(feature = "ssr")]
mod service;
#[cfg(any(feature = "ssr", feature = "hydrate"))]
mod ui;

#[cfg(feature = "hydrate")]
mod hydrate;

#[cfg(feature = "ssr")]
pub use service::run;
