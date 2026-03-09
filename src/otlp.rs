//! OTLP JSON serialization types.
//!
//! These structs mirror the OTLP protobuf schema but serialize to JSON via serde.
//! Field names use lowerCamelCase per the OTLP JSON encoding spec.
//!
//! Reference: https://opentelemetry.io/docs/specs/otlp/#json-protobuf-encoding

use serde::Serialize;

// ── Top-level request types ─────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportLogsServiceRequest {
    pub resource_logs: Vec<ResourceLogs>,
}

// ── Log hierarchy ───────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceLogs {
    pub resource: Resource,
    pub scope_logs: Vec<ScopeLogs>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScopeLogs {
    pub scope: Scope,
    pub log_records: Vec<LogRecord>,
}

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
#[allow(clippy::enum_variant_names)]
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
