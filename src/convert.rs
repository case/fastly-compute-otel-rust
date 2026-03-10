//! Conversions from `opentelemetry` SDK types to OTLP JSON serialization types.

use crate::otlp::{OtlpAnyValue, OtlpArrayValue, OtlpKeyValue, Resource as OtlpResource};
use opentelemetry::{Key, Value};
use std::time::SystemTime;

#[cfg(feature = "logs")]
use crate::otlp::OtlpKeyValueList;
#[cfg(feature = "logs")]
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(feature = "logs")]
use opentelemetry::logs::AnyValue;

/// Convert a `SystemTime` to nanoseconds-since-epoch as a decimal string.
pub(crate) fn system_time_to_nanos(t: SystemTime) -> String {
    let dur = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let nanos = dur.as_secs() as u128 * 1_000_000_000 + dur.subsec_nanos() as u128;
    nanos.to_string()
}

/// Convert an OTel log `AnyValue` to an OTLP JSON `AnyValue`.
#[cfg(feature = "logs")]
pub(crate) fn any_value_to_otlp(v: &AnyValue) -> OtlpAnyValue {
    match v {
        AnyValue::Int(i) => OtlpAnyValue::IntValue(i.to_string()),
        AnyValue::Double(f) => OtlpAnyValue::DoubleValue(*f),
        AnyValue::String(s) => OtlpAnyValue::StringValue(s.to_string()),
        AnyValue::Boolean(b) => OtlpAnyValue::BoolValue(*b),
        AnyValue::Bytes(bytes) => OtlpAnyValue::BytesValue(STANDARD.encode(bytes.as_ref())),
        AnyValue::ListAny(list) => OtlpAnyValue::ArrayValue(OtlpArrayValue {
            values: list.iter().map(any_value_to_otlp).collect(),
        }),
        AnyValue::Map(map) => OtlpAnyValue::KvlistValue(OtlpKeyValueList {
            values: map
                .iter()
                .map(|(k, v)| OtlpKeyValue {
                    key: k.as_str().to_string(),
                    value: any_value_to_otlp(v),
                })
                .collect(),
        }),
        _ => OtlpAnyValue::StringValue(format!("{v:?}")),
    }
}

/// Convert an OTel resource/span `Value` to an OTLP JSON `AnyValue`.
///
/// `Value` (used by `Resource` and span attributes) is a different enum from
/// `AnyValue` (used by log record attributes). Both map to the same OTLP
/// `AnyValue` JSON representation.
pub(crate) fn value_to_otlp(v: &Value) -> OtlpAnyValue {
    match v {
        Value::Bool(b) => OtlpAnyValue::BoolValue(*b),
        Value::I64(i) => OtlpAnyValue::IntValue(i.to_string()),
        Value::F64(f) => OtlpAnyValue::DoubleValue(*f),
        Value::String(s) => OtlpAnyValue::StringValue(s.to_string()),
        Value::Array(arr) => OtlpAnyValue::ArrayValue(OtlpArrayValue {
            values: match arr {
                opentelemetry::Array::Bool(v) => {
                    v.iter().map(|b| OtlpAnyValue::BoolValue(*b)).collect()
                }
                opentelemetry::Array::I64(v) => v
                    .iter()
                    .map(|i| OtlpAnyValue::IntValue(i.to_string()))
                    .collect(),
                opentelemetry::Array::F64(v) => {
                    v.iter().map(|f| OtlpAnyValue::DoubleValue(*f)).collect()
                }
                opentelemetry::Array::String(v) => v
                    .iter()
                    .map(|s| OtlpAnyValue::StringValue(s.to_string()))
                    .collect(),
                _ => vec![OtlpAnyValue::StringValue(format!("{arr:?}"))],
            },
        }),
        _ => OtlpAnyValue::StringValue(format!("{v:?}")),
    }
}

/// Convert an OTel log attribute `(Key, AnyValue)` pair to an OTLP KeyValue.
#[cfg(feature = "logs")]
pub(crate) fn log_attribute_to_otlp(key: &Key, value: &AnyValue) -> OtlpKeyValue {
    OtlpKeyValue {
        key: key.as_str().to_string(),
        value: any_value_to_otlp(value),
    }
}

/// Convert an OTel resource attribute `(&Key, &Value)` pair to an OTLP KeyValue.
pub(crate) fn resource_attribute_to_otlp(key: &Key, value: &Value) -> OtlpKeyValue {
    OtlpKeyValue {
        key: key.as_str().to_string(),
        value: value_to_otlp(value),
    }
}

/// Convert an `InstrumentationScope` to an OTLP `Scope`.
pub(crate) fn scope_to_otlp(scope: &opentelemetry::InstrumentationScope) -> crate::otlp::Scope {
    crate::otlp::Scope {
        name: scope.name().to_string(),
        version: scope.version().map(String::from),
        schema_url: scope.schema_url().map(String::from),
        attributes: scope
            .attributes()
            .map(|kv| resource_attribute_to_otlp(&kv.key, &kv.value))
            .collect(),
    }
}

/// Convert an `opentelemetry_sdk::Resource` to an OTLP Resource.
pub(crate) fn resource_to_otlp(resource: &opentelemetry_sdk::Resource) -> OtlpResource {
    OtlpResource {
        attributes: resource
            .iter()
            .map(|(k, v)| resource_attribute_to_otlp(k, v))
            .collect(),
    }
}

// ── Trace-only conversions ──────────────────────────────────────────

/// Convert `SpanKind` to the OTLP integer representation.
///
/// OTLP defines: 0=unspecified, 1=internal, 2=server, 3=client, 4=producer, 5=consumer.
#[cfg(feature = "trace")]
pub(crate) fn span_kind_to_otlp(kind: &opentelemetry::trace::SpanKind) -> u32 {
    use opentelemetry::trace::SpanKind;
    match kind {
        SpanKind::Internal => 1,
        SpanKind::Server => 2,
        SpanKind::Client => 3,
        SpanKind::Producer => 4,
        SpanKind::Consumer => 5,
    }
}

/// Convert `Status` to an OTLP `SpanStatus`.
///
/// OTLP defines: 0=unset, 1=ok, 2=error. Only the error variant carries a message.
#[cfg(feature = "trace")]
pub(crate) fn status_to_otlp(status: &opentelemetry::trace::Status) -> crate::otlp::SpanStatus {
    use opentelemetry::trace::Status;
    match status {
        Status::Unset => crate::otlp::SpanStatus {
            code: 0,
            message: String::new(),
        },
        Status::Ok => crate::otlp::SpanStatus {
            code: 1,
            message: String::new(),
        },
        Status::Error { description } => crate::otlp::SpanStatus {
            code: 2,
            message: description.to_string(),
        },
    }
}

/// Convert an OTel `Event` to an OTLP `SpanEvent`.
#[cfg(feature = "trace")]
pub(crate) fn event_to_otlp(event: &opentelemetry::trace::Event) -> crate::otlp::SpanEvent {
    crate::otlp::SpanEvent {
        time_unix_nano: system_time_to_nanos(event.timestamp),
        name: event.name.to_string(),
        attributes: event
            .attributes
            .iter()
            .map(|kv| resource_attribute_to_otlp(&kv.key, &kv.value))
            .collect(),
        dropped_attributes_count: event.dropped_attributes_count,
    }
}

/// Convert a `TraceState` to an `Option<String>`, returning `None` if empty.
#[cfg(feature = "trace")]
fn trace_state_to_otlp(ts: &opentelemetry::trace::TraceState) -> Option<String> {
    let s = ts.header();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Convert an OTel `Link` to an OTLP `SpanLink`.
#[cfg(feature = "trace")]
pub(crate) fn link_to_otlp(link: &opentelemetry::trace::Link) -> crate::otlp::SpanLink {
    crate::otlp::SpanLink {
        trace_id: link.span_context.trace_id().to_string(),
        span_id: link.span_context.span_id().to_string(),
        attributes: link
            .attributes
            .iter()
            .map(|kv| resource_attribute_to_otlp(&kv.key, &kv.value))
            .collect(),
        dropped_attributes_count: link.dropped_attributes_count,
        flags: link.span_context.trace_flags().to_u8() as u32,
        trace_state: trace_state_to_otlp(link.span_context.trace_state()),
    }
}

/// Convert a `SpanData` to an OTLP `Span`.
#[cfg(feature = "trace")]
pub(crate) fn span_data_to_otlp(span: &opentelemetry_sdk::trace::SpanData) -> crate::otlp::Span {
    use opentelemetry::trace::SpanId;

    let parent_span_id = if span.parent_span_id == SpanId::INVALID {
        None
    } else {
        Some(span.parent_span_id.to_string())
    };

    crate::otlp::Span {
        trace_id: span.span_context.trace_id().to_string(),
        span_id: span.span_context.span_id().to_string(),
        parent_span_id,
        name: span.name.to_string(),
        kind: span_kind_to_otlp(&span.span_kind),
        start_time_unix_nano: system_time_to_nanos(span.start_time),
        end_time_unix_nano: system_time_to_nanos(span.end_time),
        attributes: span
            .attributes
            .iter()
            .map(|kv| resource_attribute_to_otlp(&kv.key, &kv.value))
            .collect(),
        dropped_attributes_count: span.dropped_attributes_count,
        events: span.events.events.iter().map(event_to_otlp).collect(),
        dropped_events_count: span.events.dropped_count,
        links: span.links.links.iter().map(link_to_otlp).collect(),
        dropped_links_count: span.links.dropped_count,
        status: status_to_otlp(&span.status),
        flags: span.span_context.trace_flags().to_u8() as u32,
        trace_state: trace_state_to_otlp(span.span_context.trace_state()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn nanosecond_timestamp_includes_subsecond_precision() {
        let t = SystemTime::UNIX_EPOCH + Duration::new(1_696_435_200, 123_456_789);
        assert_eq!(system_time_to_nanos(t), "1696435200123456789");
    }

    #[cfg(feature = "logs")]
    #[test]
    fn int64_serialized_as_decimal_string_per_otlp_spec() {
        // OTLP JSON encodes int64 as a string, not a number — easy to get wrong
        let v = AnyValue::Int(42);
        let json = serde_json::to_string(&any_value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"intValue":"42"}"#);
    }

    #[cfg(feature = "logs")]
    #[test]
    fn bytes_serialized_as_base64() {
        // bytesValue uses standard base64 per protobuf JSON mapping.
        // Only traceId/spanId use hex.
        let v = AnyValue::Bytes(Box::new(vec![0xde, 0xad, 0xbe, 0xef]));
        let json = serde_json::to_string(&any_value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"bytesValue":"3q2+7w=="}"#);
    }

    #[cfg(feature = "logs")]
    #[test]
    fn nested_list_preserves_structure() {
        let v = AnyValue::ListAny(Box::new(vec![AnyValue::Int(1), AnyValue::Int(2)]));
        let json = serde_json::to_string(&any_value_to_otlp(&v)).unwrap();
        assert_eq!(
            json,
            r#"{"arrayValue":{"values":[{"intValue":"1"},{"intValue":"2"}]}}"#
        );
    }

    #[test]
    fn resource_roundtrips_through_otlp_json() {
        // Tests the full pipeline: OTel Resource → OTLP JSON
        let resource = opentelemetry_sdk::Resource::builder_empty()
            .with_attributes([opentelemetry::KeyValue::new("service.name", "test-svc")])
            .build();
        let json = serde_json::to_string(&resource_to_otlp(&resource)).unwrap();
        assert_eq!(
            json,
            r#"{"attributes":[{"key":"service.name","value":{"stringValue":"test-svc"}}]}"#
        );
    }

    #[test]
    fn value_bool_serialized_correctly() {
        let v = Value::Bool(true);
        let json = serde_json::to_string(&value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"boolValue":true}"#);
    }

    #[test]
    fn value_i64_serialized_as_decimal_string() {
        // Same OTLP rule as AnyValue::Int — int64 must be a string
        let v = Value::I64(-99);
        let json = serde_json::to_string(&value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"intValue":"-99"}"#);
    }

    #[test]
    fn value_f64_serialized_as_number() {
        let v = Value::F64(3.14);
        let json = serde_json::to_string(&value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"doubleValue":3.14}"#);
    }

    #[test]
    fn value_array_of_strings() {
        let v = Value::Array(opentelemetry::Array::String(vec!["a".into(), "b".into()]));
        let json = serde_json::to_string(&value_to_otlp(&v)).unwrap();
        assert_eq!(
            json,
            r#"{"arrayValue":{"values":[{"stringValue":"a"},{"stringValue":"b"}]}}"#
        );
    }

    #[test]
    fn value_array_of_ints() {
        let v = Value::Array(opentelemetry::Array::I64(vec![1, 2, 3]));
        let json = serde_json::to_string(&value_to_otlp(&v)).unwrap();
        assert_eq!(
            json,
            r#"{"arrayValue":{"values":[{"intValue":"1"},{"intValue":"2"},{"intValue":"3"}]}}"#
        );
    }

    #[cfg(feature = "trace")]
    #[test]
    fn span_kind_maps_to_otlp_integers() {
        use opentelemetry::trace::SpanKind;
        assert_eq!(span_kind_to_otlp(&SpanKind::Internal), 1);
        assert_eq!(span_kind_to_otlp(&SpanKind::Server), 2);
        assert_eq!(span_kind_to_otlp(&SpanKind::Client), 3);
        assert_eq!(span_kind_to_otlp(&SpanKind::Producer), 4);
        assert_eq!(span_kind_to_otlp(&SpanKind::Consumer), 5);
    }

    #[cfg(feature = "trace")]
    #[test]
    fn status_unset_serializes_with_code_zero() {
        let s = status_to_otlp(&opentelemetry::trace::Status::Unset);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"code":0}"#);
    }

    #[cfg(feature = "trace")]
    #[test]
    fn status_ok_serializes_with_code_one() {
        let s = status_to_otlp(&opentelemetry::trace::Status::Ok);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"code":1}"#);
    }

    #[cfg(feature = "trace")]
    #[test]
    fn status_error_includes_message() {
        let s = status_to_otlp(&opentelemetry::trace::Status::Error {
            description: "something broke".into(),
        });
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"code":2,"message":"something broke"}"#);
    }

    /// Build a minimal `SpanData` for testing. Callers can override fields after creation.
    #[cfg(feature = "trace")]
    fn make_test_span(
        parent_span_id: opentelemetry::trace::SpanId,
    ) -> opentelemetry_sdk::trace::SpanData {
        use opentelemetry::trace::{SpanContext, SpanId, SpanKind, Status, TraceFlags, TraceId};
        use opentelemetry_sdk::trace::{SpanData, SpanEvents, SpanLinks};
        use std::borrow::Cow;

        SpanData {
            span_context: SpanContext::new(
                TraceId::from_hex("0af7651916cd43dd8448eb211c80319c").unwrap(),
                SpanId::from_hex("b7ad6b7169203331").unwrap(),
                TraceFlags::SAMPLED,
                false,
                Default::default(),
            ),
            parent_span_id,
            parent_span_is_remote: false,
            span_kind: SpanKind::Server,
            name: Cow::Borrowed("GET /api/users"),
            start_time: SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 0),
            end_time: SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 50_000_000),
            attributes: vec![opentelemetry::KeyValue::new("http.method", "GET")],
            dropped_attributes_count: 0,
            events: SpanEvents::default(),
            links: SpanLinks::default(),
            status: Status::Ok,
            instrumentation_scope: opentelemetry::InstrumentationScope::builder("test-tracer")
                .with_version("0.1.0")
                .build(),
        }
    }

    #[cfg(feature = "trace")]
    #[test]
    fn span_data_to_otlp_produces_valid_structure() {
        use opentelemetry::trace::SpanId;

        let span = make_test_span(SpanId::from_hex("00f067aa0ba902b7").unwrap());

        let otlp_span = span_data_to_otlp(&span);
        let json: serde_json::Value = serde_json::to_value(&otlp_span).unwrap();

        assert_eq!(json["traceId"], "0af7651916cd43dd8448eb211c80319c");
        assert_eq!(json["spanId"], "b7ad6b7169203331");
        assert_eq!(json["parentSpanId"], "00f067aa0ba902b7");
        assert_eq!(json["name"], "GET /api/users");
        assert_eq!(json["kind"], 2); // SERVER
        assert_eq!(json["startTimeUnixNano"], "1700000000000000000");
        assert_eq!(json["endTimeUnixNano"], "1700000000050000000");
        assert_eq!(json["status"]["code"], 1); // OK
        assert_eq!(json["attributes"][0]["key"], "http.method");
        assert_eq!(json["attributes"][0]["value"]["stringValue"], "GET");
        // Dropped counts and empty collections should be omitted
        assert!(json.get("droppedAttributesCount").is_none());
        assert!(json.get("events").is_none());
        assert!(json.get("links").is_none());
        // traceState should be omitted when empty
        assert!(json.get("traceState").is_none());
    }

    #[cfg(feature = "trace")]
    #[test]
    fn root_span_omits_parent_span_id() {
        use opentelemetry::trace::SpanId;

        let span = make_test_span(SpanId::INVALID);

        let otlp_span = span_data_to_otlp(&span);
        let json: serde_json::Value = serde_json::to_value(&otlp_span).unwrap();

        assert!(json.get("parentSpanId").is_none());
    }

    #[cfg(feature = "trace")]
    #[test]
    fn span_event_serializes_correctly() {
        let event = opentelemetry::trace::Event::new(
            "exception",
            SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 100_000),
            vec![opentelemetry::KeyValue::new(
                "exception.message",
                "not found",
            )],
            0,
        );

        let otlp_event = event_to_otlp(&event);
        let json: serde_json::Value = serde_json::to_value(&otlp_event).unwrap();

        assert_eq!(json["name"], "exception");
        assert_eq!(json["timeUnixNano"], "1700000000000100000");
        assert_eq!(json["attributes"][0]["key"], "exception.message");
    }

    #[cfg(feature = "trace")]
    #[test]
    fn span_link_serializes_correctly() {
        use opentelemetry::trace::{Link, SpanContext, SpanId, TraceFlags, TraceId};

        let linked_context = SpanContext::new(
            TraceId::from_hex("aaaabbbbccccdddd1111222233334444").unwrap(),
            SpanId::from_hex("eeee5555ffff6666").unwrap(),
            TraceFlags::SAMPLED,
            false,
            Default::default(),
        );

        let link = Link::new(
            linked_context,
            vec![opentelemetry::KeyValue::new("link.reason", "retry")],
            0,
        );

        let otlp_link = link_to_otlp(&link);
        let json: serde_json::Value = serde_json::to_value(&otlp_link).unwrap();

        assert_eq!(json["traceId"], "aaaabbbbccccdddd1111222233334444");
        assert_eq!(json["spanId"], "eeee5555ffff6666");
        assert_eq!(json["attributes"][0]["key"], "link.reason");
        assert_eq!(json["attributes"][0]["value"]["stringValue"], "retry");
        // traceState should be omitted when empty
        assert!(json.get("traceState").is_none());
    }
}
