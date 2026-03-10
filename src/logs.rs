//! Fastly log endpoint exporter for OpenTelemetry logs.
//!
//! Implements `LogExporter` by serializing `SdkLogRecord`s to OTLP JSON
//! and writing them to a Fastly named log endpoint.

use crate::convert;
use crate::otlp;
use crate::FastlyOtelError;
use opentelemetry::InstrumentationScope;
use opentelemetry_sdk::logs::{LogBatch, LogExporter};
use opentelemetry_sdk::Resource;
use std::fmt;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// Exports OTel log records as OTLP JSON to a Fastly named log endpoint.
pub struct FastlyLogExporter {
    endpoint_name: String,
    resource: Resource,
    is_shutdown: AtomicBool,
}

impl fmt::Debug for FastlyLogExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FastlyLogExporter")
            .field("endpoint_name", &self.endpoint_name)
            .finish()
    }
}

impl FastlyLogExporter {
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

    /// Serialize a batch of log records to OTLP JSON and write to the log endpoint.
    fn export_sync(&self, batch: LogBatch<'_>) -> Result<(), FastlyOtelError> {
        let mut endpoint =
            fastly::log::Endpoint::try_from_name(&self.endpoint_name).map_err(|e| {
                FastlyOtelError::EndpointOpen {
                    name: self.endpoint_name.clone(),
                    source: std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()),
                }
            })?;

        // Group log records by instrumentation scope (name + version).
        let mut scope_map: Vec<(&InstrumentationScope, Vec<otlp::LogRecord>)> = Vec::new();

        for (record, scope) in batch.iter() {
            let log_record = otlp::LogRecord {
                time_unix_nano: record.timestamp().map(convert::system_time_to_nanos),
                observed_time_unix_nano: record
                    .observed_timestamp()
                    .map(convert::system_time_to_nanos),
                severity_number: record.severity_number().map(|s| s as u32),
                severity_text: record.severity_text().map(String::from),
                body: record.body().map(convert::any_value_to_otlp),
                attributes: record
                    .attributes_iter()
                    .map(|(k, v)| convert::log_attribute_to_otlp(k, v))
                    .collect(),
                trace_id: record.trace_context().map(|tc| tc.trace_id.to_string()),
                span_id: record.trace_context().map(|tc| tc.span_id.to_string()),
                flags: record
                    .trace_context()
                    .and_then(|tc| tc.trace_flags.map(|f| f.to_u8() as u32)),
            };

            // Find existing scope group or create a new one.
            if let Some(entry) = scope_map.iter_mut().find(|(s, _)| *s == scope) {
                entry.1.push(log_record);
            } else {
                scope_map.push((scope, vec![log_record]));
            }
        }

        // Build the OTLP request with one ScopeLogs per instrumentation scope.
        let scope_logs: Vec<otlp::ScopeLogs> = scope_map
            .into_iter()
            .map(|(scope, log_records)| otlp::ScopeLogs {
                scope: convert::scope_to_otlp(scope),
                log_records,
            })
            .collect();

        let request = otlp::ExportLogsServiceRequest {
            resource_logs: vec![otlp::ResourceLogs {
                resource: convert::resource_to_otlp(&self.resource),
                scope_logs,
            }],
        };

        // Serialize to compact JSON — one write per request.
        let json = serde_json::to_string(&request)?;
        writeln!(endpoint, "{json}").map_err(FastlyOtelError::Write)?;

        Ok(())
    }
}

impl LogExporter for FastlyLogExporter {
    async fn export(&self, batch: LogBatch<'_>) -> opentelemetry_sdk::error::OTelSdkResult {
        if self.is_shutdown.load(Ordering::Relaxed) {
            return Err(opentelemetry_sdk::error::OTelSdkError::AlreadyShutdown);
        }

        self.export_sync(batch).map_err(|e| {
            opentelemetry_sdk::error::OTelSdkError::InternalFailure(format!(
                "FastlyLogExporter: {e}"
            ))
        })
    }

    fn shutdown_with_timeout(
        &self,
        _timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.is_shutdown.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.resource = resource.clone();
    }
}
