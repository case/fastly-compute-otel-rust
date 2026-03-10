//! W3C Trace Context propagation for Fastly Compute requests.
//!
//! Provides [`Injector`] and [`Extractor`] adapters that bridge the
//! `opentelemetry` propagation API to Fastly's `Request` header methods,
//! plus convenience functions for common propagation patterns.

use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::propagation::TraceContextPropagator;

/// Adapter that implements [`Injector`] for a mutable `fastly::Request`.
///
/// Used to inject W3C `traceparent` and `tracestate` headers into outgoing
/// backend fetch requests.
///
/// # Example
///
/// ```ignore
/// use opentelemetry_sdk::propagation::TraceContextPropagator;
/// use opentelemetry::propagation::TextMapPropagator;
/// use fastly_compute_otel::propagation::FastlyRequestInjector;
///
/// let propagator = TraceContextPropagator::new();
/// let mut backend_req = fastly::Request::get("https://origin.example.com/");
/// propagator.inject_context(&cx, &mut FastlyRequestInjector(&mut backend_req));
/// ```
pub struct FastlyRequestInjector<'a>(pub &'a mut fastly::Request);

impl Injector for FastlyRequestInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        self.0.set_header(key, value);
    }
}

/// Adapter that implements [`Extractor`] for a `fastly::Request` reference.
///
/// Used to extract W3C `traceparent` and `tracestate` headers from incoming
/// edge requests.
///
/// # Example
///
/// ```ignore
/// use opentelemetry_sdk::propagation::TraceContextPropagator;
/// use opentelemetry::propagation::TextMapPropagator;
/// use fastly_compute_otel::propagation::FastlyRequestExtractor;
///
/// let propagator = TraceContextPropagator::new();
/// let parent_cx = propagator.extract(&FastlyRequestExtractor(&req));
/// ```
pub struct FastlyRequestExtractor<'a>(pub &'a fastly::Request);

impl<'a> Extractor for FastlyRequestExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        // Use `get_header` + manual UTF-8 conversion instead of `get_header_str`,
        // which panics on non-UTF-8 values. Returning `None` for invalid UTF-8
        // matches the `opentelemetry-http` HeaderExtractor behavior.
        self.0
            .get_header(key)
            .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.get_header_names_str()
    }
}

/// Inject the current span's trace context into an outgoing `fastly::Request`.
///
/// Adds `traceparent` (and `tracestate` if present) headers so the downstream
/// service can continue the distributed trace.
///
/// Uses the provided [`Context`] to determine which span's trace/span IDs to
/// propagate. Typically called with the context returned by
/// [`extract_context`] or constructed via `trace::set_span()`.
pub fn inject_context(cx: &Context, req: &mut fastly::Request) {
    let propagator = TraceContextPropagator::new();
    propagator.inject_context(cx, &mut FastlyRequestInjector(req));
}

/// Extract a parent trace context from an incoming `fastly::Request`.
///
/// Reads the `traceparent` (and `tracestate`) headers and returns a new
/// [`Context`] carrying the remote [`SpanContext`]. If no valid trace context
/// is found, returns the current context unchanged.
///
/// The returned context should be passed as the parent when starting the
/// root span for this request.
pub fn extract_context(req: &fastly::Request) -> Context {
    let propagator = TraceContextPropagator::new();
    propagator.extract(&FastlyRequestExtractor(req))
}

/// Read the Fastly-assigned request ID from the environment.
///
/// Returns a [`KeyValue`] suitable for adding as a span attribute:
/// `fastly.request_id = "<value>"`.
///
/// Fastly sets the `FASTLY_TRACE_ID` environment variable for every Compute
/// request. This is Fastly's own correlation ID (unrelated to W3C trace IDs)
/// and is useful for cross-referencing with Fastly's dashboard and support.
///
/// Returns `None` if the variable is not set (e.g. running outside Fastly Compute).
pub fn fastly_request_id() -> Option<KeyValue> {
    std::env::var("FASTLY_TRACE_ID")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|id| KeyValue::new("fastly.request_id", id))
}

/// Check whether a [`Context`] carries a valid remote span context.
///
/// Useful after [`extract_context`] to determine whether the incoming request
/// had a `traceparent` header.
pub fn has_remote_span_context(cx: &Context) -> bool {
    use opentelemetry::trace::TraceContextExt;
    let span = cx.span();
    let sc = span.span_context();
    sc.is_valid() && sc.is_remote()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // These unit tests run natively (no WASI runtime). Integration tests that
    // exercise `fastly::Request` header injection/extraction run under Viceroy.

    #[test]
    fn fastly_request_id_returns_none_when_unset() {
        // In the test environment, FASTLY_TRACE_ID is not set.
        // This may or may not be set depending on the test runner,
        // so we test the filter logic via the function contract.
        let result = fastly_request_id();
        // Outside Fastly Compute, this should be None.
        assert!(result.is_none());
    }

    #[test]
    fn propagator_roundtrips_through_hashmap() {
        // Verify the TraceContextPropagator works with the standard HashMap
        // Injector/Extractor (provided by opentelemetry). This validates our
        // understanding of the API without needing Fastly's Request type.
        use opentelemetry::trace::{
            SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
        };

        let trace_id = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
        let span_id = SpanId::from_hex("b7ad6b7169203331").unwrap();
        let span_context = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            true, // remote
            TraceState::default(),
        );
        let parent_cx = Context::new().with_remote_span_context(span_context);

        // Inject into a HashMap
        let propagator = TraceContextPropagator::new();
        let mut carrier = HashMap::new();
        propagator.inject_context(&parent_cx, &mut carrier);

        assert!(carrier.contains_key("traceparent"));
        let traceparent = &carrier["traceparent"];
        assert!(traceparent.contains("0af7651916cd43dd8448eb211c80319c"));
        assert!(traceparent.contains("b7ad6b7169203331"));

        // Extract back from the HashMap
        let extracted_cx = propagator.extract(&carrier);
        let extracted_sc = extracted_cx.span().span_context().clone();
        assert_eq!(extracted_sc.trace_id(), trace_id);
        assert!(extracted_sc.is_remote());
        assert!(extracted_sc.trace_flags().is_sampled());
    }
}
