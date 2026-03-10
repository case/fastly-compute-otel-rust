//! OTLP JSON serialization types.
//!
//! These structs mirror the OTLP protobuf schema but serialize to JSON via serde.
//! Field names use lowerCamelCase per the OTLP JSON encoding spec.
//!
//! Reference: https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding

use serde::Serialize;

// ── Log request types ───────────────────────────────────────────────

#[cfg(feature = "logs")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportLogsServiceRequest {
    pub resource_logs: Vec<ResourceLogs>,
}

#[cfg(feature = "logs")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceLogs {
    pub resource: Resource,
    pub scope_logs: Vec<ScopeLogs>,
}

#[cfg(feature = "logs")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScopeLogs {
    pub scope: Scope,
    pub log_records: Vec<LogRecord>,
}

#[cfg(feature = "logs")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogRecord {
    /// Nanosecond timestamp as a decimal string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_unix_nano: Option<String>,

    /// Nanosecond observed timestamp as a decimal string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_time_unix_nano: Option<String>,

    /// Severity number (integer, not enum name).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity_number: Option<u32>,

    /// Severity text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity_text: Option<String>,

    /// Log body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<OtlpAnyValue>,

    /// Log attributes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<OtlpKeyValue>,

    /// Trace ID as hex string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,

    /// Span ID as hex string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,

    /// Trace flags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
}

// ── Shared types ────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Resource {
    pub attributes: Vec<OtlpKeyValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Scope {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<OtlpKeyValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OtlpKeyValue {
    pub key: String,
    pub value: OtlpAnyValue,
}

/// OTLP AnyValue — a oneof where exactly one field is present.
/// Variant names match OTLP JSON field names (serialized via serde).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::enum_variant_names, dead_code)]
pub(crate) enum OtlpAnyValue {
    StringValue(String),
    BoolValue(bool),
    /// int64 encoded as decimal string per OTLP JSON spec.
    IntValue(String),
    DoubleValue(f64),
    ArrayValue(OtlpArrayValue),
    KvlistValue(OtlpKeyValueList),
    /// base64-encoded bytes per standard protobuf JSON mapping.
    BytesValue(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OtlpArrayValue {
    pub values: Vec<OtlpAnyValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OtlpKeyValueList {
    pub values: Vec<OtlpKeyValue>,
}

// ── Trace hierarchy ──────────────────────────────────────────────────

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportTraceServiceRequest {
    pub resource_spans: Vec<ResourceSpans>,
}

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceSpans {
    pub resource: Resource,
    pub scope_spans: Vec<ScopeSpans>,
}

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScopeSpans {
    pub scope: Scope,
    pub spans: Vec<Span>,
}

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Span {
    /// Hex-encoded 16-byte trace ID.
    pub trace_id: String,

    /// Hex-encoded 8-byte span ID.
    pub span_id: String,

    /// Hex-encoded 8-byte parent span ID (omitted for root spans).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,

    /// Human-readable span name.
    pub name: String,

    /// Span kind (OTLP integer: 0=unspecified, 1=internal, 2=server, 3=client, 4=producer, 5=consumer).
    pub kind: u32,

    /// Start time as nanoseconds since epoch (decimal string).
    pub start_time_unix_nano: String,

    /// End time as nanoseconds since epoch (decimal string).
    pub end_time_unix_nano: String,

    /// Span attributes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<OtlpKeyValue>,

    /// Number of attributes that were dropped due to limits.
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_attributes_count: u32,

    /// Timed events within the span.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<SpanEvent>,

    /// Number of events that were dropped due to limits.
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_events_count: u32,

    /// Links to other spans.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<SpanLink>,

    /// Number of links that were dropped due to limits.
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_links_count: u32,

    /// Span status.
    pub status: SpanStatus,

    /// W3C trace flags (lower 8 bits).
    #[serde(skip_serializing_if = "is_zero")]
    pub flags: u32,

    /// W3C tracestate header, serialized as a string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_state: Option<String>,
}

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpanEvent {
    /// Event timestamp as nanoseconds since epoch (decimal string).
    pub time_unix_nano: String,

    /// Event name.
    pub name: String,

    /// Event attributes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<OtlpKeyValue>,

    /// Number of attributes that were dropped.
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_attributes_count: u32,
}

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpanLink {
    /// Hex-encoded trace ID of the linked span.
    pub trace_id: String,

    /// Hex-encoded span ID of the linked span.
    pub span_id: String,

    /// Link attributes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<OtlpKeyValue>,

    /// Number of attributes that were dropped.
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_attributes_count: u32,

    /// Trace flags of the linked span.
    #[serde(skip_serializing_if = "is_zero")]
    pub flags: u32,

    /// W3C tracestate header of the linked span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_state: Option<String>,
}

#[cfg(feature = "trace")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpanStatus {
    /// Status code (0=unset, 1=ok, 2=error).
    pub code: u32,

    /// Optional status message (only meaningful with error status).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub message: String,
}

#[cfg(feature = "trace")]
fn is_zero(v: &u32) -> bool {
    *v == 0
}
