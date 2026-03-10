//! OpenTelemetry log and trace export for Fastly Compute,
//! using named log providers as transport.
//!
//! This crate adapts the upstream `opentelemetry` SDK for Fastly Compute's
//! WASI environment, where the standard `opentelemetry-otlp` exporter cannot
//! compile (it depends on `tonic`/`reqwest` which require threads and async runtimes).
//!
//! Instead, this crate serializes OTLP JSON and writes it to Fastly named log
//! endpoints (`fastly::log::Endpoint`), which stream the data to any configured
//! HTTPS receiver.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use thiserror::Error;

#[cfg(any(feature = "logs", feature = "trace"))]
mod convert;
#[cfg(any(feature = "logs", feature = "trace"))]
mod otlp;

#[cfg(any(feature = "logs", feature = "trace"))]
mod init;
#[cfg(feature = "logs")]
mod logs;
#[cfg(feature = "trace")]
mod traces;

#[cfg(any(feature = "logs", feature = "trace"))]
pub use init::{FastlyOtel, FastlyOtelBuilder};
#[cfg(feature = "logs")]
pub use logs::FastlyLogExporter;
#[cfg(feature = "trace")]
pub use traces::FastlySpanExporter;

/// Errors that can occur during OTel export on Fastly Compute.
#[derive(Debug, Error)]
pub enum FastlyOtelError {
    /// The named log endpoint could not be opened (e.g. not configured in `fastly.toml`).
    #[error("failed to open log endpoint '{name}': {source}")]
    EndpointOpen {
        /// The endpoint name that was requested.
        name: String,
        /// The underlying I/O error from the Fastly runtime.
        source: std::io::Error,
    },

    /// OTLP JSON serialization failed.
    #[error("failed to serialize OTLP JSON: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Writing serialized telemetry to the log endpoint failed.
    #[error("failed to write to log endpoint: {0}")]
    Write(std::io::Error),

    /// Builder configuration is invalid (e.g. missing required fields).
    #[error("configuration error: {0}")]
    Config(&'static str),
}
