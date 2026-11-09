//! Conversion `storm_replay::Value` → `serde_json::Value`, in the representation of the JS port
//! of heroprotocol used by hots-parser: blobs = UTF-8 strings, structs = objects.

use serde_json::Value as J;
use storm_replay::Value;

pub fn jval(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Int(i) => J::from(*i),
        Value::Bool(b) => J::from(*b),
        Value::Real(f) => serde_json::Number::from_f64(*f).map_or(J::Null, J::Number),
        Value::Blob(b) => J::from(String::from_utf8_lossy(b).into_owned()),
        Value::Str(s) => J::from(s.as_ref()),
        Value::Fourcc(b) => J::from(String::from_utf8_lossy(b).into_owned()),
        Value::Array(items) => J::Array(items.iter().map(jval).collect()),
        // never read by hots-parser — neutral shapes
        Value::BitArrayBytes { bits, data } => {
            serde_json::json!([bits, String::from_utf8_lossy(data)])
        }
        Value::BitArrayInt { bits, .. } => serde_json::json!([bits, J::Null]),
        Value::Struct(fields) => J::Object(
            fields
                .iter()
                .map(|(n, v)| (n.to_string(), jval(v)))
                .collect(),
        ),
    }
}
