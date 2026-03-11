//! Integration tests running under Viceroy (Fastly's local Compute runtime).
//!
//! These tests are skipped when compiling for the native target (`cargo test`).
//! They require `--target wasm32-wasip1` so Viceroy provides the Fastly hostcalls.

#![cfg(target_arch = "wasm32")]
//!
//! These verify the full library API works in a real WASI environment where
//! Fastly hostcalls (log endpoints, request headers) are available.
//!
//! Run with:
//!   cargo nextest run --target wasm32-wasip1 --test viceroy
//! Or:
//!   cargo test --target wasm32-wasip1 --test viceroy

use fastly::http::StatusCode;
use fastly_compute_otel::FastlyOtel;

// ---------------------------------------------------------------------------
// Builder + provider construction
// ---------------------------------------------------------------------------

#[cfg(feature = "logs")]
#[test]
fn build_with_log_endpoint() {
    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .log_endpoint("otel")
        .build()
        .expect("build with log endpoint should succeed");

    assert!(otel.logger_provider().is_some());
    otel.shutdown().expect("shutdown should succeed");
}

#[cfg(feature = "trace")]
#[test]
fn build_with_trace_endpoint() {
    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build()
        .expect("build with trace endpoint should succeed");

    assert!(otel.tracer_provider().is_some());
    otel.shutdown().expect("shutdown should succeed");
}

#[cfg(all(feature = "trace", feature = "logs"))]
#[test]
fn build_with_shared_endpoint() {
    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .endpoint("otel")
        .build()
        .expect("build with shared endpoint should succeed");

    assert!(otel.tracer_provider().is_some());
    assert!(otel.logger_provider().is_some());
    otel.shutdown().expect("shutdown should succeed");
}

#[cfg(all(feature = "trace", feature = "logs"))]
#[test]
fn build_with_custom_resource_attributes() {
    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .service_namespace("test-team")
        .service_version("1.2.3")
        .deployment_environment("test")
        .resource_attribute(opentelemetry::KeyValue::new("custom.attr", "value"))
        .endpoint("otel")
        .build()
        .expect("build with custom attributes should succeed");

    otel.shutdown().expect("shutdown should succeed");
}

// ---------------------------------------------------------------------------
// Trace export lifecycle
// ---------------------------------------------------------------------------

#[cfg(feature = "trace")]
#[test]
fn span_create_and_export() {
    use opentelemetry::trace::{Tracer, TracerProvider};

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build()
        .expect("build should succeed");

    let tp = otel
        .tracer_provider()
        .expect("tracer provider should exist");
    let tracer = tp.tracer("test-tracer");

    // Creating and dropping a span triggers export via SimpleSpanProcessor.
    let span = tracer.start("test-span");
    drop(span);

    otel.shutdown().expect("shutdown should succeed");
}

#[cfg(feature = "trace")]
#[test]
fn span_with_attributes_and_events() {
    use opentelemetry::trace::{Span, Tracer, TracerProvider};
    use opentelemetry::KeyValue;

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build()
        .expect("build should succeed");

    let tp = otel
        .tracer_provider()
        .expect("tracer provider should exist");
    let tracer = tp.tracer("test-tracer");

    let mut span = tracer.start("span-with-attrs");
    span.set_attribute(KeyValue::new("http.request.method", "GET"));
    span.set_attribute(KeyValue::new("url.path", "/test"));
    span.add_event("cache-lookup", vec![KeyValue::new("cache.hit", false)]);
    span.end();

    otel.shutdown().expect("shutdown should succeed");
}

// ---------------------------------------------------------------------------
// Log export lifecycle
// ---------------------------------------------------------------------------

#[cfg(feature = "logs")]
#[test]
fn log_record_create_and_export() {
    use opentelemetry::logs::{LogRecord, Logger, LoggerProvider, Severity};

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .log_endpoint("otel")
        .build()
        .expect("build should succeed");

    let lp = otel
        .logger_provider()
        .expect("logger provider should exist");
    let logger = lp.logger("test-logger");

    let mut record = logger.create_log_record();
    record.set_severity_number(Severity::Info);
    record.set_severity_text("INFO");
    record.set_body("integration test log message".into());
    logger.emit(record);

    otel.shutdown().expect("shutdown should succeed");
}

#[cfg(feature = "logs")]
#[test]
fn log_record_with_attributes() {
    use opentelemetry::logs::{LogRecord, Logger, LoggerProvider, Severity};

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .log_endpoint("otel")
        .build()
        .expect("build should succeed");

    let lp = otel
        .logger_provider()
        .expect("logger provider should exist");
    let logger = lp.logger("test-logger");

    let mut record = logger.create_log_record();
    record.set_severity_number(Severity::Warn);
    record.set_severity_text("WARN");
    record.set_body("cache miss".into());
    record.add_attribute("url.path", "/api/data");
    record.add_attribute("fastly.cache_status", "MISS");
    logger.emit(record);

    otel.shutdown().expect("shutdown should succeed");
}

// ---------------------------------------------------------------------------
// build_from_request — root span creation
// ---------------------------------------------------------------------------

#[cfg(feature = "trace")]
#[test]
fn build_from_request_creates_root_span() {
    let req = fastly::Request::get("https://example.com/test-path")
        .with_header("host", "example.com")
        .with_header("user-agent", "integration-test/1.0");

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build_from_request(&req)
        .expect("build_from_request should succeed");

    assert!(otel.root_context().is_some());
    otel.shutdown().expect("shutdown should succeed");
}

#[cfg(feature = "trace")]
#[test]
fn build_from_request_continues_incoming_trace() {
    use opentelemetry::trace::TraceContextExt;

    let req = fastly::Request::get("https://example.com/test")
        .with_header("host", "example.com")
        .with_header(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        );

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build_from_request(&req)
        .expect("build_from_request with traceparent should succeed");

    let root_cx = otel.root_context().expect("root context should exist");
    let sc = root_cx.span().span_context().clone();

    // The root span should continue the incoming distributed trace.
    assert_eq!(
        sc.trace_id().to_string(),
        "0af7651916cd43dd8448eb211c80319c",
        "root span should carry the incoming trace_id"
    );

    otel.shutdown().expect("shutdown should succeed");
}

// ---------------------------------------------------------------------------
// traceparent propagation on real Fastly Requests
// ---------------------------------------------------------------------------

#[cfg(feature = "trace")]
#[test]
fn traceparent_injection_into_fastly_request() {
    use opentelemetry::trace::{
        SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
    };

    let trace_id = TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap();
    let span_id = SpanId::from_hex("b7ad6b7169203331").unwrap();
    let span_context = SpanContext::new(
        trace_id,
        span_id,
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    );
    let cx = opentelemetry::Context::new().with_remote_span_context(span_context);

    let mut req = fastly::Request::get("https://origin.example.com/");
    fastly_compute_otel::propagation::inject_context(&cx, &mut req);

    let traceparent = req
        .get_header_str("traceparent")
        .expect("traceparent header should be set");
    assert!(
        traceparent.contains("0af7651916cd43dd8448eb211c80319c"),
        "traceparent should contain trace_id: {traceparent}"
    );
    assert!(
        traceparent.contains("b7ad6b7169203331"),
        "traceparent should contain span_id: {traceparent}"
    );
}

#[cfg(feature = "trace")]
#[test]
fn traceparent_extraction_from_fastly_request() {
    use opentelemetry::trace::TraceContextExt;

    let req = fastly::Request::get("https://example.com/").with_header(
        "traceparent",
        "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
    );

    let cx = fastly_compute_otel::propagation::extract_context(&req);
    let sc = cx.span().span_context().clone();

    assert!(sc.is_valid(), "extracted span context should be valid");
    assert!(sc.is_remote(), "extracted span context should be remote");
    assert_eq!(
        sc.trace_id().to_string(),
        "0af7651916cd43dd8448eb211c80319c"
    );
}

#[cfg(feature = "trace")]
#[test]
fn traceparent_roundtrip_through_fastly_request() {
    use opentelemetry::trace::{
        SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
    };

    let trace_id = TraceId::from_hex("abcdef1234567890abcdef1234567890").unwrap();
    let span_id = SpanId::from_hex("1234567890abcdef").unwrap();
    let original = SpanContext::new(
        trace_id,
        span_id,
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    );
    let cx = opentelemetry::Context::new().with_remote_span_context(original);

    // Inject into a Fastly Request.
    let mut req = fastly::Request::get("https://origin.example.com/api");
    fastly_compute_otel::propagation::inject_context(&cx, &mut req);

    // Extract from the same Request — should roundtrip.
    let extracted_cx = fastly_compute_otel::propagation::extract_context(&req);
    let extracted_sc = extracted_cx.span().span_context().clone();

    assert_eq!(extracted_sc.trace_id(), trace_id);
    assert!(extracted_sc.is_remote());
    assert!(extracted_sc.trace_flags().is_sampled());
}

// ---------------------------------------------------------------------------
// FASTLY_TRACE_ID environment variable
// ---------------------------------------------------------------------------

#[cfg(feature = "trace")]
#[test]
fn fastly_request_id_in_viceroy() {
    // Viceroy sets FASTLY_TRACE_ID for `serve` mode but may not for `run` mode.
    // This test verifies the function doesn't panic regardless of whether the
    // variable is set. If Viceroy does set it, we verify the KeyValue format.
    let result = fastly_compute_otel::propagation::fastly_request_id();
    if let Some(kv) = result {
        assert_eq!(kv.key.as_str(), "fastly.request_id");
        assert!(!kv.value.as_str().is_empty());
    }
    // No assertion on None — it's valid for `run` mode to not set the variable.
}

// ---------------------------------------------------------------------------
// finish() — full request lifecycle
// ---------------------------------------------------------------------------

#[cfg(feature = "trace")]
#[test]
fn finish_records_status_and_shuts_down() {
    let req = fastly::Request::get("https://example.com/test").with_header("host", "example.com");

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build_from_request(&req)
        .expect("build_from_request should succeed");

    let resp =
        fastly::Response::from_status(StatusCode::OK).with_header("content-type", "text/plain");

    let resp = otel.finish(resp).expect("finish should succeed");
    assert_eq!(resp.get_status(), StatusCode::OK);
}

#[cfg(feature = "trace")]
#[test]
fn finish_marks_error_on_5xx() {
    let req = fastly::Request::get("https://example.com/error");

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .trace_endpoint("otel")
        .build_from_request(&req)
        .expect("build_from_request should succeed");

    let resp = fastly::Response::from_status(StatusCode::INTERNAL_SERVER_ERROR);

    // finish() should succeed even on error responses — it records the status
    // on the span but doesn't fail the response.
    let resp = otel
        .finish(resp)
        .expect("finish should succeed on error responses");
    assert_eq!(resp.get_status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// ---------------------------------------------------------------------------
// Combined trace + log lifecycle
// ---------------------------------------------------------------------------

#[cfg(all(feature = "trace", feature = "logs"))]
#[test]
fn full_lifecycle_traces_and_logs() {
    use opentelemetry::logs::{LogRecord, Logger, LoggerProvider, Severity};
    use opentelemetry::trace::{Span, Tracer, TracerProvider};
    use opentelemetry::KeyValue;

    let req = fastly::Request::get("https://example.com/api/data")
        .with_header("host", "example.com")
        .with_header("user-agent", "integration-test/1.0");

    let otel = FastlyOtel::builder()
        .service_name("integration-test")
        .service_version("0.1.0")
        .endpoint("otel")
        .build_from_request(&req)
        .expect("build_from_request should succeed");

    // Emit a custom child span.
    let tp = otel
        .tracer_provider()
        .expect("tracer provider should exist");
    let tracer = tp.tracer("test-app");

    let root_cx = otel.root_context().expect("root context should exist");
    let mut child = tracer.start_with_context("process-request", root_cx);
    child.set_attribute(KeyValue::new("custom.attr", "value"));
    child.end();

    // Emit a log record.
    let lp = otel
        .logger_provider()
        .expect("logger provider should exist");
    let logger = lp.logger("test-app");

    let mut record = logger.create_log_record();
    record.set_severity_number(Severity::Info);
    record.set_body("request processed successfully".into());
    logger.emit(record);

    // Finalize.
    let resp = fastly::Response::from_status(StatusCode::OK);
    let resp = otel.finish(resp).expect("finish should succeed");
    assert_eq!(resp.get_status(), StatusCode::OK);
}

// ---------------------------------------------------------------------------
// Graceful degradation — export errors must not crash the request
// ---------------------------------------------------------------------------
// NOTE: Viceroy creates log endpoints dynamically for any name, so we cannot
// trigger FastlyOtelError::EndpointOpen here. In production Fastly, referencing
// an endpoint name not configured in the service dashboard would fail at
// try_from_name(). These tests verify the higher-level guarantee: telemetry
// failures (from the OTel SDK's perspective) don't propagate to the caller.

#[cfg(feature = "trace")]
#[test]
fn shutdown_after_export_does_not_panic() {
    // Verify that the full create-span-shutdown cycle completes without panic,
    // even if the exporter encountered issues internally.
    use opentelemetry::trace::{Tracer, TracerProvider};

    let otel = FastlyOtel::builder()
        .service_name("degradation-test")
        .trace_endpoint("otel")
        .build()
        .expect("build should succeed");

    let tp = otel
        .tracer_provider()
        .expect("tracer provider should exist");
    let tracer = tp.tracer("test-tracer");

    // Create and immediately drop multiple spans — exercises the exporter
    // under rapid sequential export.
    for i in 0..5 {
        let span = tracer.start(format!("span-{i}"));
        drop(span);
    }

    // Shutdown should succeed without propagating any export errors.
    otel.shutdown()
        .expect("shutdown after rapid export should succeed");
}

#[cfg(all(feature = "trace", feature = "logs"))]
#[test]
fn finish_returns_response_even_after_double_shutdown() {
    // Verify that calling shutdown() then finish() doesn't panic or lose
    // the response. This covers the edge case where a user calls shutdown()
    // explicitly on an error path, then later calls finish() in cleanup.
    let req = fastly::Request::get("https://example.com/double-shutdown");

    let otel = FastlyOtel::builder()
        .service_name("degradation-test")
        .endpoint("otel")
        .build_from_request(&req)
        .expect("build should succeed");

    // First shutdown — explicit, on an "error path".
    let _ = otel.shutdown();

    // Second shutdown via finish — should still return the response.
    let resp = fastly::Response::from_status(StatusCode::OK);
    let resp = otel
        .finish(resp)
        .expect("finish after shutdown should still return response");
    assert_eq!(resp.get_status(), StatusCode::OK);
}

#[cfg(feature = "trace")]
#[test]
fn send_to_nonexistent_backend_returns_error_without_panic() {
    // When a backend fetch fails (e.g., backend not declared in fastly.toml),
    // otel.send() should return the error cleanly — the child span should be
    // ended with error status, and no panic should occur.
    let req = fastly::Request::get("https://example.com/api");

    let otel = FastlyOtel::builder()
        .service_name("degradation-test")
        .trace_endpoint("otel")
        .build_from_request(&req)
        .expect("build should succeed");

    let backend_req = fastly::Request::get("https://nonexistent.example.com/");
    let result = otel.send(backend_req, "nonexistent-backend");

    // The send should fail (backend not configured in Viceroy), but not panic.
    assert!(
        result.is_err(),
        "send to nonexistent backend should return an error"
    );

    // finish() should still work — the root span should be finalized normally.
    let resp = fastly::Response::from_status(StatusCode::BAD_GATEWAY);
    let resp = otel
        .finish(resp)
        .expect("finish should succeed even after send error");
    assert_eq!(resp.get_status(), StatusCode::BAD_GATEWAY);
}
