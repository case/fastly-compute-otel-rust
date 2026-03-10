//! Initialization and configuration API for Fastly Compute OTel.
//!
//! Provides a builder that wires up `TracerProvider` and/or `LoggerProvider`
//! with the Fastly log endpoint exporters in a single call.

use crate::FastlyOtelError;
use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;

/// Configured OTel instance for a Fastly Compute request.
///
/// Created via [`FastlyOtel::builder()`]. Holds the configured providers
/// and handles clean shutdown at the end of the request.
#[derive(Debug)]
pub struct FastlyOtel {
    #[cfg(feature = "trace")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    #[cfg(feature = "logs")]
    logger_provider: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
}

impl FastlyOtel {
    /// Create a new builder for configuring OTel on Fastly Compute.
    #[must_use]
    pub fn builder() -> FastlyOtelBuilder {
        FastlyOtelBuilder {
            service_name: None,
            service_version: None,
            resource_attributes: Vec::new(),
            #[cfg(feature = "trace")]
            trace_endpoint: None,
            #[cfg(feature = "logs")]
            log_endpoint: None,
        }
    }

    /// Get the tracer provider, if traces are enabled.
    #[cfg(feature = "trace")]
    pub fn tracer_provider(&self) -> Option<&opentelemetry_sdk::trace::SdkTracerProvider> {
        self.tracer_provider.as_ref()
    }

    /// Get the logger provider, if logs are enabled.
    #[cfg(feature = "logs")]
    pub fn logger_provider(&self) -> Option<&opentelemetry_sdk::logs::SdkLoggerProvider> {
        self.logger_provider.as_ref()
    }

    /// Shut down all configured providers, flushing any pending telemetry.
    ///
    /// Call this at the end of the request handler, before returning the response.
    /// Fastly Compute is request-scoped — there is no background processing after
    /// the response is sent.
    ///
    /// Attempts to shut down all providers even if one fails. Returns the first
    /// error encountered, if any.
    pub fn shutdown(&self) -> Result<(), opentelemetry_sdk::error::OTelSdkError> {
        let mut first_error = None;

        #[cfg(feature = "trace")]
        if let Some(tp) = &self.tracer_provider {
            if let Err(e) = tp.shutdown() {
                first_error = Some(e);
            }
        }
        #[cfg(feature = "logs")]
        if let Some(lp) = &self.logger_provider {
            if let Err(e) = lp.shutdown() {
                first_error.get_or_insert(e);
            }
        }

        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Builder for [`FastlyOtel`].
///
/// At minimum, set a service name and at least one endpoint (log or trace).
///
/// # Example (with default features: `trace` + `logs`)
///
/// ```ignore
/// use fastly_compute_otel::FastlyOtel;
///
/// let otel = FastlyOtel::builder()
///     .service_name("my-edge-app")
///     .log_endpoint("otel-endpoint")
///     .trace_endpoint("otel-endpoint")
///     .build()
///     .expect("failed to initialize OTel");
/// ```
#[derive(Debug)]
pub struct FastlyOtelBuilder {
    service_name: Option<String>,
    service_version: Option<String>,
    resource_attributes: Vec<KeyValue>,
    #[cfg(feature = "trace")]
    trace_endpoint: Option<String>,
    #[cfg(feature = "logs")]
    log_endpoint: Option<String>,
}

impl FastlyOtelBuilder {
    /// Set the `service.name` resource attribute (required).
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }

    /// Set the `service.version` resource attribute.
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = Some(version.into());
        self
    }

    /// Add a custom resource attribute.
    pub fn resource_attribute(mut self, kv: impl Into<KeyValue>) -> Self {
        self.resource_attributes.push(kv.into());
        self
    }

    /// Set the named log endpoint for trace export.
    ///
    /// Setting this enables trace export. The endpoint name must match one
    /// configured in `fastly.toml` (local dev) or the Fastly dashboard.
    #[cfg(feature = "trace")]
    pub fn trace_endpoint(mut self, name: impl Into<String>) -> Self {
        self.trace_endpoint = Some(name.into());
        self
    }

    /// Set the named log endpoint for log export.
    ///
    /// Setting this enables log export. The endpoint name must match one
    /// configured in `fastly.toml` (local dev) or the Fastly dashboard.
    #[cfg(feature = "logs")]
    pub fn log_endpoint(mut self, name: impl Into<String>) -> Self {
        self.log_endpoint = Some(name.into());
        self
    }

    /// Check that the builder has the minimum required configuration.
    ///
    /// This is separated from `build()` so validation logic can be tested
    /// natively without linking against the Fastly WASI runtime.
    fn validate_preconditions(&self) -> Result<(), FastlyOtelError> {
        if self.service_name.is_none() {
            return Err(FastlyOtelError::Config("service_name is required"));
        }

        // Check that at least one signal is configured.
        #[cfg(all(feature = "trace", feature = "logs"))]
        let has_endpoint = self.trace_endpoint.is_some() || self.log_endpoint.is_some();
        #[cfg(all(feature = "trace", not(feature = "logs")))]
        let has_endpoint = self.trace_endpoint.is_some();
        #[cfg(all(feature = "logs", not(feature = "trace")))]
        let has_endpoint = self.log_endpoint.is_some();
        #[cfg(not(any(feature = "trace", feature = "logs")))]
        let has_endpoint = false;

        if !has_endpoint {
            return Err(FastlyOtelError::Config(
                "at least one endpoint must be configured (log_endpoint or trace_endpoint)",
            ));
        }

        Ok(())
    }

    /// Build the shared OTel `Resource` from owned builder fields.
    fn build_resource(
        service_name: String,
        service_version: Option<String>,
        mut resource_attributes: Vec<KeyValue>,
    ) -> Resource {
        let mut attrs = vec![KeyValue::new("service.name", service_name)];
        if let Some(version) = service_version {
            attrs.push(KeyValue::new("service.version", version));
        }
        attrs.append(&mut resource_attributes);
        Resource::builder_empty().with_attributes(attrs).build()
    }

    /// Build the configured OTel instance.
    ///
    /// Returns an error if `service_name` is not set or no endpoints are configured.
    pub fn build(self) -> Result<FastlyOtel, FastlyOtelError> {
        self.validate_preconditions()?;

        // Safe to unwrap: validate_preconditions checked service_name is Some.
        let resource = Self::build_resource(
            self.service_name.unwrap(),
            self.service_version,
            self.resource_attributes,
        );

        // Build tracer provider if trace endpoint is configured.
        #[cfg(feature = "trace")]
        let tracer_provider = self.trace_endpoint.map(|endpoint_name| {
            opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_simple_exporter(crate::FastlySpanExporter::new(endpoint_name))
                .with_resource(resource.clone())
                .build()
        });

        // Build logger provider if log endpoint is configured.
        #[cfg(feature = "logs")]
        let logger_provider = self.log_endpoint.map(|endpoint_name| {
            opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                .with_simple_exporter(crate::FastlyLogExporter::new(endpoint_name))
                .with_resource(resource.clone())
                .build()
        });

        Ok(FastlyOtel {
            #[cfg(feature = "trace")]
            tracer_provider,
            #[cfg(feature = "logs")]
            logger_provider,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests call `validate_preconditions()` and `build_resource()` directly,
    // which are pure logic that never touches Fastly FFI. They run natively
    // without a WASI runtime.

    #[cfg(feature = "logs")]
    #[test]
    fn validate_fails_without_service_name() {
        let builder = FastlyOtel::builder().log_endpoint("otel");
        let err = builder.validate_preconditions().unwrap_err();
        assert!(err.to_string().contains("service_name"));
    }

    #[test]
    fn validate_fails_without_any_endpoint() {
        let builder = FastlyOtel::builder().service_name("test-svc");
        let err = builder.validate_preconditions().unwrap_err();
        assert!(err.to_string().contains("endpoint"));
    }

    #[cfg(feature = "logs")]
    #[test]
    fn validate_succeeds_with_log_endpoint() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .log_endpoint("otel");
        assert!(builder.validate_preconditions().is_ok());
    }

    #[cfg(feature = "trace")]
    #[test]
    fn validate_succeeds_with_trace_endpoint() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .trace_endpoint("otel");
        assert!(builder.validate_preconditions().is_ok());
    }

    #[cfg(all(feature = "trace", feature = "logs"))]
    #[test]
    fn validate_succeeds_with_both_endpoints() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .log_endpoint("otel-logs")
            .trace_endpoint("otel-traces");
        assert!(builder.validate_preconditions().is_ok());
    }

    #[test]
    fn build_resource_includes_service_version_and_custom_attributes() {
        let resource = FastlyOtelBuilder::build_resource(
            "test-svc".to_string(),
            Some("1.2.3".to_string()),
            vec![KeyValue::new("deployment.environment", "staging")],
        );

        let attrs: Vec<_> = resource.iter().collect();
        assert!(attrs.iter().any(|(k, _)| k.as_str() == "service.name"));
        assert!(attrs.iter().any(|(k, _)| k.as_str() == "service.version"));
        assert!(attrs
            .iter()
            .any(|(k, _)| k.as_str() == "deployment.environment"));
    }

    // Success-path tests that construct providers (build()) require WASI because
    // the `fastly` crate links against host-provided FFI symbols.
    // These will run under Viceroy in Phase 7 integration tests.
}
