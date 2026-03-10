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

use thiserror::Error;

#[cfg(any(feature = "logs", feature = "trace"))]
mod convert;
#[cfg(any(feature = "logs", feature = "trace"))]
mod otlp;

#[cfg(feature = "logs")]
mod logs;
#[cfg(feature = "trace")]
mod traces;

#[cfg(feature = "logs")]
pub use logs::FastlyLogExporter;
#[cfg(feature = "trace")]
pub use traces::FastlySpanExporter;

/// Errors that can occur during OTel export on Fastly Compute.
#[derive(Debug, Error)]
pub enum FastlyOtelError {
    #[error("failed to open log endpoint '{name}': {source}")]
    EndpointOpen {
        name: String,
        source: std::io::Error,
    },

    #[error("failed to serialize OTLP JSON: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("failed to write to log endpoint: {0}")]
    Write(std::io::Error),
}
