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
    pub fn shutdown(&self) {
        #[cfg(feature = "trace")]
        if let Some(tp) = &self.tracer_provider {
            let _ = tp.shutdown();
        }
        #[cfg(feature = "logs")]
        if let Some(lp) = &self.logger_provider {
            let _ = lp.shutdown();
        }
    }
}

/// Builder for [`FastlyOtel`].
///
/// At minimum, set a service name and at least one endpoint (log or trace).
///
/// # Example
///
/// ```no_run
/// use fastly_compute_otel::FastlyOtel;
///
/// let otel = FastlyOtel::builder()
///     .service_name("my-edge-app")
///     .log_endpoint("otel-endpoint")
///     .trace_endpoint("otel-endpoint")
///     .build()
///     .expect("failed to initialize OTel");
/// ```
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
    pub fn resource_attribute(mut self, kv: KeyValue) -> Self {
        self.resource_attributes.push(kv);
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

    /// Validate the builder configuration without constructing providers.
    ///
    /// Returns the validated config as a `Resource` and endpoint names.
    /// This is separated from `build()` so validation logic can be tested
    /// natively without linking against the Fastly WASI runtime.
    fn validate(&self) -> Result<ValidatedConfig, FastlyOtelError> {
        let service_name = self
            .service_name
            .as_ref()
            .ok_or(FastlyOtelError::Config("service_name is required"))?;

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

        // Build the shared resource.
        let mut attrs = vec![KeyValue::new("service.name", service_name.clone())];
        if let Some(version) = &self.service_version {
            attrs.push(KeyValue::new("service.version", version.clone()));
        }
        attrs.extend(self.resource_attributes.clone());
        let resource = Resource::builder_empty().with_attributes(attrs).build();

        Ok(ValidatedConfig { resource })
    }

    /// Build the configured OTel instance.
    ///
    /// Returns an error if `service_name` is not set or no endpoints are configured.
    pub fn build(self) -> Result<FastlyOtel, FastlyOtelError> {
        let config = self.validate()?;

        // Build tracer provider if trace endpoint is configured.
        #[cfg(feature = "trace")]
        let tracer_provider = self.trace_endpoint.map(|endpoint_name| {
            opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_simple_exporter(crate::FastlySpanExporter::new(endpoint_name))
                .with_resource(config.resource.clone())
                .build()
        });

        // Build logger provider if log endpoint is configured.
        #[cfg(feature = "logs")]
        let logger_provider = self.log_endpoint.map(|endpoint_name| {
            opentelemetry_sdk::logs::SdkLoggerProvider::builder()
                .with_simple_exporter(crate::FastlyLogExporter::new(endpoint_name))
                .with_resource(config.resource.clone())
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

/// Validated builder configuration, ready to construct providers.
#[derive(Debug)]
struct ValidatedConfig {
    resource: Resource,
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests call `validate()` directly, which is pure logic that never
    // touches Fastly FFI. They run natively without a WASI runtime.

    #[test]
    fn validate_fails_without_service_name() {
        let builder = FastlyOtel::builder().log_endpoint("otel");
        let err = builder.validate().unwrap_err();
        assert!(err.to_string().contains("service_name"));
    }

    #[test]
    fn validate_fails_without_any_endpoint() {
        let builder = FastlyOtel::builder().service_name("test-svc");
        let err = builder.validate().unwrap_err();
        assert!(err.to_string().contains("endpoint"));
    }

    #[test]
    fn validate_succeeds_with_log_endpoint() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .log_endpoint("otel");
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn validate_succeeds_with_trace_endpoint() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .trace_endpoint("otel");
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn validate_succeeds_with_both_endpoints() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .log_endpoint("otel-logs")
            .trace_endpoint("otel-traces");
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn validate_includes_service_version_and_custom_attributes() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .service_version("1.2.3")
            .resource_attribute(KeyValue::new("deployment.environment", "staging"))
            .log_endpoint("otel");
        let config = builder.validate().unwrap();

        // Verify all attributes made it into the resource.
        let attrs: Vec<_> = config.resource.iter().collect();
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
