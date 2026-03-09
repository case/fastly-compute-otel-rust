//! Conversions from `opentelemetry` SDK types to OTLP JSON serialization types.

use crate::otlp::{
    OtlpAnyValue, OtlpArrayValue, OtlpKeyValue, OtlpKeyValueList, Resource as OtlpResource,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use opentelemetry::logs::AnyValue;
use opentelemetry::{Key, Value};
use std::time::SystemTime;

/// Convert a `SystemTime` to nanoseconds-since-epoch as a decimal string.
pub(crate) fn system_time_to_nanos(t: SystemTime) -> String {
    let dur = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let nanos = dur.as_secs() as u128 * 1_000_000_000 + dur.subsec_nanos() as u128;
    nanos.to_string()
}

/// Convert an OTel log `AnyValue` to an OTLP JSON `AnyValue`.
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

/// Convert an `opentelemetry_sdk::Resource` to an OTLP Resource.
pub(crate) fn resource_to_otlp(resource: &opentelemetry_sdk::Resource) -> OtlpResource {
    OtlpResource {
        attributes: resource
            .iter()
            .map(|(k, v)| resource_attribute_to_otlp(k, v))
            .collect(),
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

    #[test]
    fn int64_serialized_as_decimal_string_per_otlp_spec() {
        // OTLP JSON encodes int64 as a string, not a number — easy to get wrong
        let v = AnyValue::Int(42);
        let json = serde_json::to_string(&any_value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"intValue":"42"}"#);
    }

    #[test]
    fn bytes_serialized_as_base64() {
        // bytesValue uses standard base64 per protobuf JSON mapping.
        // Only traceId/spanId use hex.
        let v = AnyValue::Bytes(Box::new(vec![0xde, 0xad, 0xbe, 0xef]));
        let json = serde_json::to_string(&any_value_to_otlp(&v)).unwrap();
        assert_eq!(json, r#"{"bytesValue":"3q2+7w=="}"#);
    }

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
}
