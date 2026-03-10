//! Fastly log endpoint exporter for OpenTelemetry traces.
//!
//! Implements `SpanExporter` by serializing `SpanData` to OTLP JSON
//! and writing it to a Fastly named log endpoint.

use crate::convert;
use crate::otlp;
use crate::FastlyOtelError;
use opentelemetry::InstrumentationScope;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use opentelemetry_sdk::Resource;
use std::fmt;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Exports OTel spans as OTLP JSON to a Fastly named log endpoint.
pub struct FastlySpanExporter {
    endpoint_name: String,
    resource: Resource,
    is_shutdown: AtomicBool,
}

impl fmt::Debug for FastlySpanExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FastlySpanExporter")
            .field("endpoint_name", &self.endpoint_name)
            .finish()
    }
}

impl FastlySpanExporter {
    /// Create a new exporter that writes OTLP JSON to the named log endpoint.
    ///
    /// The endpoint name must match a log endpoint configured in `fastly.toml`
    /// (local dev) or the Fastly service dashboard (production).
    pub fn new(endpoint_name: impl Into<String>) -> Self {
        Self {
            endpoint_name: endpoint_name.into(),
            resource: Resource::builder().build(),
            is_shutdown: AtomicBool::new(false),
        }
    }

    /// Serialize a batch of spans to OTLP JSON and write to the log endpoint.
    ///
    /// Spans are grouped by instrumentation scope, then emitted as a single
    /// `ExportTraceServiceRequest` JSON line per batch.
    fn export_sync(&self, batch: Vec<SpanData>) -> Result<(), FastlyOtelError> {
        if batch.is_empty() {
            return Ok(());
        }

        let mut endpoint =
            fastly::log::Endpoint::try_from_name(&self.endpoint_name).map_err(|e| {
                FastlyOtelError::EndpointOpen {
                    name: self.endpoint_name.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()),
                }
            })?;

        // Group spans by instrumentation scope (name + version).
        let mut scope_map: Vec<(&InstrumentationScope, Vec<otlp::Span>)> = Vec::new();

        for span in &batch {
            let scope = &span.instrumentation_scope;
            let otlp_span = convert::span_data_to_otlp(span);

            // Find existing scope group or create a new one.
            if let Some(entry) = scope_map.iter_mut().find(|(s, _)| *s == scope) {
                entry.1.push(otlp_span);
            } else {
                scope_map.push((scope, vec![otlp_span]));
            }
        }

        // Build the OTLP request with one ScopeSpans per instrumentation scope.
        let scope_spans: Vec<otlp::ScopeSpans> = scope_map
            .into_iter()
            .map(|(scope, spans)| otlp::ScopeSpans {
                scope: otlp::Scope {
                    name: scope.name().to_string(),
                    version: scope.version().map(String::from),
                    schema_url: scope.schema_url().map(String::from),
                    attributes: scope
                        .attributes()
                        .map(|kv| convert::resource_attribute_to_otlp(&kv.key, &kv.value))
                        .collect(),
                },
                spans,
            })
            .collect();

        let request = otlp::ExportTraceServiceRequest {
            resource_spans: vec![otlp::ResourceSpans {
                resource: convert::resource_to_otlp(&self.resource),
                scope_spans,
            }],
        };

        // Serialize to compact JSON — one write per batch.
        let json = serde_json::to_string(&request)?;
        writeln!(endpoint, "{json}").map_err(FastlyOtelError::Write)?;

        Ok(())
    }
}

impl SpanExporter for FastlySpanExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = opentelemetry_sdk::error::OTelSdkResult> + Send {
        let result = if self.is_shutdown.load(Ordering::Relaxed) {
            Err(opentelemetry_sdk::error::OTelSdkError::AlreadyShutdown)
        } else {
            self.export_sync(batch).map_err(|e| {
                opentelemetry_sdk::error::OTelSdkError::InternalFailure(format!(
                    "FastlySpanExporter: {e}"
                ))
            })
        };
        std::future::ready(result)
    }

    fn shutdown_with_timeout(
        &mut self,
        _timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.is_shutdown.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.resource = resource.clone();
    }
}
