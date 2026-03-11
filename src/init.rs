//! Initialization and configuration API for Fastly Compute OTel.
//!
//! Provides a builder that wires up `TracerProvider` and/or `LoggerProvider`
//! with the Fastly log endpoint exporters in a single call.

use crate::FastlyOtelError;
use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;

/// Configured OTel instance for a Fastly Compute request.
///
/// Created via [`FastlyOtel::builder()`]. Holds the configured providers,
/// the root span context, and handles clean shutdown at the end of the request.
///
/// # Typical usage
///
/// ```ignore
/// use fastly::{Error, Request, Response};
/// use fastly_compute_otel::FastlyOtel;
///
/// #[fastly::main]
/// fn main(req: Request) -> Result<Response, Error> {
///     let otel = FastlyOtel::builder()
///         .service_name("my-edge-app")
///         .service_namespace("my-team")
///         .service_version(env!("CARGO_PKG_VERSION"))
///         .deployment_environment("production")
///         .endpoint("otel-endpoint")
///         .build_from_request(&req)?;
///
///     let mut bereq = req.clone_with_body();
///     let beresp = otel.send(bereq, "origin")?;
///
///     otel.finish(beresp)
/// }
/// ```
pub struct FastlyOtel {
    #[cfg(feature = "trace")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    #[cfg(feature = "logs")]
    logger_provider: Option<opentelemetry_sdk::logs::SdkLoggerProvider>,
    #[cfg(feature = "trace")]
    root_cx: Option<opentelemetry::Context>,
}

impl std::fmt::Debug for FastlyOtel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("FastlyOtel");
        #[cfg(feature = "trace")]
        d.field("has_tracer_provider", &self.tracer_provider.is_some());
        #[cfg(feature = "logs")]
        d.field("has_logger_provider", &self.logger_provider.is_some());
        #[cfg(feature = "trace")]
        d.field("has_root_span", &self.root_cx.is_some());
        d.finish()
    }
}

impl FastlyOtel {
    /// Create a new builder for configuring OTel on Fastly Compute.
    #[must_use]
    pub fn builder() -> FastlyOtelBuilder {
        FastlyOtelBuilder {
            service_name: None,
            service_namespace: None,
            service_version: None,
            deployment_environment: None,
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

    /// Get the root span context, if a root span was created via
    /// [`FastlyOtelBuilder::build_from_request`].
    ///
    /// Use this as the parent context when creating custom spans:
    ///
    /// ```ignore
    /// let tracer = otel.tracer_provider().unwrap().tracer("my-app");
    /// let span = tracer.start_with_context("custom-work", otel.root_context().unwrap());
    /// ```
    #[cfg(feature = "trace")]
    pub fn root_context(&self) -> Option<&opentelemetry::Context> {
        self.root_cx.as_ref()
    }

    /// Record an HTTP status code on the span in `cx`, mark as error if >= 500, and end the span.
    #[cfg(feature = "trace")]
    fn finish_span(cx: &opentelemetry::Context, status_code: u16) {
        use opentelemetry::trace::TraceContextExt;

        cx.span().set_attribute(KeyValue::new(
            "http.response.status_code",
            i64::from(status_code),
        ));
        if status_code >= 500 {
            cx.span().set_status(opentelemetry::trace::Status::Error {
                description: format!("HTTP {status_code}").into(),
            });
        }
        cx.span().end();
    }

    /// Send a request to a backend with automatic trace instrumentation.
    ///
    /// Creates a child span under the root span, injects W3C `traceparent`
    /// into the outgoing request headers, sends the request, and records the
    /// response status code on the child span.
    ///
    /// If tracing is not configured, this is equivalent to calling
    /// `req.send(backend)` directly.
    pub fn send(
        &self,
        #[cfg_attr(not(feature = "trace"), allow(unused_mut))] mut req: fastly::Request,
        backend: &str,
    ) -> Result<fastly::Response, fastly::Error> {
        #[cfg(feature = "trace")]
        if let (Some(tp), Some(root_cx)) = (&self.tracer_provider, &self.root_cx) {
            use opentelemetry::trace::{SpanKind, TraceContextExt, Tracer, TracerProvider};

            let tracer = tp.tracer("fastly-compute-otel");

            let attrs = vec![
                KeyValue::new("http.request.method", req.get_method_str().to_owned()),
                KeyValue::new("url.full", req.get_url_str().to_owned()),
                KeyValue::new("server.address", backend.to_owned()),
            ];

            let span = tracer
                .span_builder(format!("backend {backend}"))
                .with_kind(SpanKind::Client)
                .with_attributes(attrs)
                .start_with_context(&tracer, root_cx);

            let child_cx = root_cx.with_span(span);

            crate::propagation::inject_context(&child_cx, &mut req);

            return match req.send(backend) {
                Ok(resp) => {
                    Self::finish_span(&child_cx, resp.get_status().as_u16());
                    Ok(resp)
                }
                Err(e) => {
                    use opentelemetry::trace::TraceContextExt;
                    child_cx
                        .span()
                        .set_status(opentelemetry::trace::Status::Error {
                            description: e.to_string().into(),
                        });
                    child_cx.span().end();
                    Err(e.into())
                }
            };
        }

        // No trace provider — send without instrumentation.
        Ok(req.send(backend)?)
    }

    /// Finalize the root span and shut down all providers.
    ///
    /// Records the response status code on the root span, ends it, and flushes
    /// all pending telemetry. Returns the response unchanged.
    ///
    /// This consumes `self` to prevent accidental use after shutdown. Use this
    /// as the final expression in your `#[fastly::main]` handler:
    ///
    /// ```ignore
    /// otel.finish(beresp)
    /// ```
    pub fn finish(self, resp: fastly::Response) -> Result<fastly::Response, fastly::Error> {
        #[cfg(feature = "trace")]
        if let Some(root_cx) = &self.root_cx {
            Self::finish_span(root_cx, resp.get_status().as_u16());
        }

        // Shut down providers — swallow errors so we never fail the response
        // due to telemetry issues.
        let _ = self.shutdown();

        Ok(resp)
    }

    /// Shut down all configured providers, flushing any pending telemetry.
    ///
    /// Prefer [`finish`](Self::finish) for the typical case — it records
    /// response info on the root span before shutting down. Use `shutdown`
    /// directly only if you need to flush without a response (e.g., on error
    /// paths where you construct a synthetic response).
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
///     .service_namespace("my-team")
///     .service_version("1.0.0")
///     .deployment_environment("production")
///     .endpoint("otel-endpoint")
///     .build()
///     .expect("failed to initialize OTel");
/// ```
#[derive(Debug)]
pub struct FastlyOtelBuilder {
    service_name: Option<String>,
    service_namespace: Option<String>,
    service_version: Option<String>,
    deployment_environment: Option<String>,
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

    /// Set the `service.namespace` resource attribute.
    ///
    /// A namespace for [`service.name`](Self::service_name). Use this to group
    /// related services — for example, by team or product area. The service name
    /// is expected to be unique within the same namespace.
    ///
    /// See: <https://opentelemetry.io/docs/specs/semconv/resource/#service>
    pub fn service_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.service_namespace = Some(namespace.into());
        self
    }

    /// Set the `service.version` resource attribute.
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = Some(version.into());
        self
    }

    /// Set the `deployment.environment.name` resource attribute.
    ///
    /// The name of the deployment environment, such as `"production"`,
    /// `"staging"`, or `"development"`. This does not affect service identity —
    /// the same service in different environments is still considered the same
    /// service per the OTel specification.
    ///
    /// See: <https://opentelemetry.io/docs/specs/semconv/resource/deployment-environment/>
    pub fn deployment_environment(mut self, environment: impl Into<String>) -> Self {
        self.deployment_environment = Some(environment.into());
        self
    }

    /// Add a custom resource attribute.
    pub fn resource_attribute(mut self, kv: impl Into<KeyValue>) -> Self {
        self.resource_attributes.push(kv.into());
        self
    }

    /// Set a single named log endpoint for all enabled signals (traces and logs).
    ///
    /// This is a convenience for the common case where both signals share
    /// the same Fastly named log endpoint. Equivalent to calling both
    /// [`trace_endpoint`](Self::trace_endpoint) and
    /// [`log_endpoint`](Self::log_endpoint) with the same name.
    pub fn endpoint(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        #[cfg(all(feature = "trace", feature = "logs"))]
        {
            self.trace_endpoint = Some(name.clone());
            self.log_endpoint = Some(name);
        }
        #[cfg(all(feature = "trace", not(feature = "logs")))]
        {
            self.trace_endpoint = Some(name);
        }
        #[cfg(all(feature = "logs", not(feature = "trace")))]
        {
            self.log_endpoint = Some(name);
        }
        #[cfg(not(any(feature = "trace", feature = "logs")))]
        let _ = name;
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

    /// Build the shared OTel `Resource` from the builder's current state.
    ///
    /// Caller must ensure `service_name` is `Some` (i.e., call after
    /// `validate_preconditions`).
    fn build_resource(&self) -> Resource {
        // Safe to unwrap: validate_preconditions checked service_name is Some.
        let mut attrs = vec![
            KeyValue::new("service.name", self.service_name.clone().unwrap()),
            // Auto-set telemetry SDK attributes per the OTel resource spec.
            KeyValue::new("telemetry.sdk.name", "fastly-compute-otel"),
            KeyValue::new("telemetry.sdk.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("telemetry.sdk.language", "rust"),
        ];
        if let Some(namespace) = &self.service_namespace {
            attrs.push(KeyValue::new("service.namespace", namespace.clone()));
        }
        if let Some(version) = &self.service_version {
            attrs.push(KeyValue::new("service.version", version.clone()));
        }
        if let Some(environment) = &self.deployment_environment {
            attrs.push(KeyValue::new(
                "deployment.environment.name",
                environment.clone(),
            ));
        }
        attrs.extend(self.resource_attributes.iter().cloned());
        Resource::builder_empty().with_attributes(attrs).build()
    }

    /// Build the configured OTel instance.
    ///
    /// Returns an error if `service_name` is not set or no endpoints are configured.
    #[must_use = "building the OTel instance has no effect unless the result is used"]
    pub fn build(self) -> Result<FastlyOtel, FastlyOtelError> {
        self.validate_preconditions()?;

        let resource = self.build_resource();

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
            #[cfg(feature = "trace")]
            root_cx: None,
        })
    }

    /// Build the configured OTel instance and create a root span from an incoming request.
    ///
    /// This is the recommended entry point for most Fastly Compute services.
    /// It combines [`build`](Self::build) with automatic root span creation:
    ///
    /// 1. Initializes providers (same as [`build`](Self::build))
    /// 2. Extracts any incoming W3C `traceparent` header for distributed trace continuation
    /// 3. Creates a root `SpanKind::Server` span with HTTP semantic convention attributes
    ///
    /// The root span is finalized when you call [`FastlyOtel::finish`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// let otel = FastlyOtel::builder()
    ///     .service_name("my-edge-app")
    ///     .endpoint("otel-endpoint")
    ///     .build_from_request(&req)?;
    ///
    /// let beresp = otel.send(req.clone_with_body(), "origin")?;
    /// otel.finish(beresp)
    /// ```
    #[cfg(feature = "trace")]
    #[must_use = "building the OTel instance has no effect unless the result is used"]
    pub fn build_from_request(self, req: &fastly::Request) -> Result<FastlyOtel, FastlyOtelError> {
        let mut otel = self.build()?;

        if let Some(tp) = &otel.tracer_provider {
            use opentelemetry::trace::{SpanKind, TraceContextExt, Tracer, TracerProvider};

            let parent_cx = crate::propagation::extract_context(req);

            let tracer = tp.tracer("fastly-compute-otel");

            let mut attrs = vec![
                KeyValue::new("http.request.method", req.get_method_str().to_owned()),
                KeyValue::new("url.path", req.get_path().to_owned()),
            ];

            // Safe header reads — get_header_str() panics on non-UTF-8.
            if let Some(host) = req
                .get_header("host")
                .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
            {
                attrs.push(KeyValue::new("server.address", host.to_owned()));
            }

            if let Some(ua) = req
                .get_header("user-agent")
                .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
            {
                attrs.push(KeyValue::new("user_agent.original", ua.to_owned()));
            }

            // Fastly-specific: client IP from the Fastly-Client-IP header.
            if let Some(client_ip) = req
                .get_header("fastly-client-ip")
                .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
            {
                attrs.push(KeyValue::new("client.address", client_ip.to_owned()));
            }

            // Fastly-assigned request ID for correlation with Fastly's dashboard.
            if let Some(rid) = crate::propagation::fastly_request_id() {
                attrs.push(rid);
            }

            // Use just the HTTP method as span name to avoid high-cardinality
            // from request paths. The full path is captured in `url.path`.
            let span = tracer
                .span_builder(req.get_method_str().to_owned())
                .with_kind(SpanKind::Server)
                .with_attributes(attrs)
                .start_with_context(&tracer, &parent_cx);

            otel.root_cx = Some(parent_cx.with_span(span));
        }

        Ok(otel)
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
    fn build_resource_includes_all_standard_attributes() {
        let builder = FastlyOtel::builder()
            .service_name("test-svc")
            .service_namespace("my-team")
            .service_version("1.2.3")
            .deployment_environment("staging")
            .resource_attribute(KeyValue::new("custom.attr", "value"));

        let resource = builder.build_resource();
        let attrs: Vec<_> = resource.iter().collect();

        // Helper to find attribute value by key.
        let find = |key: &str| {
            attrs
                .iter()
                .find(|(k, _)| k.as_str() == key)
                .map(|(_, v)| v.to_string())
        };

        // Required + explicit attributes
        assert_eq!(find("service.name").as_deref(), Some("test-svc"));
        assert_eq!(find("service.namespace").as_deref(), Some("my-team"));
        assert_eq!(find("service.version").as_deref(), Some("1.2.3"));
        assert_eq!(
            find("deployment.environment.name").as_deref(),
            Some("staging")
        );
        assert_eq!(find("custom.attr").as_deref(), Some("value"));

        // Auto-set telemetry SDK attributes
        assert_eq!(
            find("telemetry.sdk.name").as_deref(),
            Some("fastly-compute-otel")
        );
        assert_eq!(
            find("telemetry.sdk.version").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(find("telemetry.sdk.language").as_deref(), Some("rust"));
    }

    #[test]
    fn build_resource_omits_optional_attributes_when_not_set() {
        let builder = FastlyOtel::builder().service_name("test-svc");

        let resource = builder.build_resource();
        let attrs: Vec<_> = resource.iter().collect();
        let has = |key: &str| attrs.iter().any(|(k, _)| k.as_str() == key);

        // Always present
        assert!(has("service.name"));
        assert!(has("telemetry.sdk.name"));

        // Should NOT be present
        assert!(!has("service.namespace"));
        assert!(!has("service.version"));
        assert!(!has("deployment.environment.name"));
    }

    // Success-path tests that construct providers (build()) require WASI because
    // the `fastly` crate links against host-provided FFI symbols.
    // These will run under Viceroy in Phase 7 integration tests.
}
