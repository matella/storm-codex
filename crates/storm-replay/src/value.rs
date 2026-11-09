//! Decoded values — equivalent of the Python structures produced by heroprotocol.

/// Decoded value of a replay stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Bool(bool),
    Blob(Vec<u8>),
    /// Interned string (`_event` annotations — avoids one allocation per event).
    Str(std::sync::Arc<str>),
    Fourcc([u8; 4]),
    Real(f64),
    Array(Vec<Value>),
    /// `_bitarray` of the **versioned** decoder: aligned bytes (reference: `(length, bytes)`).
    BitArrayBytes {
        bits: u64,
        data: Vec<u8>,
    },
    /// `_bitarray` of the **bitpacked** decoder: integer of `bits` bits (reference:
    /// `(length, int)`). Stored as right-aligned big-endian bytes because it can
    /// exceed 64 bits (length encoded on 8 bits → up to 255 bits).
    BitArrayInt {
        bits: u64,
        value: Vec<u8>,
    },
    /// Structs **and** choices (a choice = a struct with a single field, as in Python).
    /// Insertion order is preserved.
    Struct(Vec<(std::sync::Arc<str>, Value)>),
}

impl Value {
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Struct(fields) => fields
                .iter()
                .find(|(n, _)| n.as_ref() == name)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_blob(&self) -> Option<&[u8]> {
        match self {
            Value::Blob(b) => Some(b),
            _ => None,
        }
    }

    /// Text blobs (names, titles…): HotS replays are in UTF-8.
    pub fn as_str_lossy(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.to_string()),
            _ => self
                .as_blob()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Value of the first field of a struct/choice (`_varuint32_value` semantics).
    pub fn first_field_int(&self) -> Option<i64> {
        match self {
            Value::Struct(fields) => fields.first().and_then(|(_, v)| v.as_int()),
            _ => None,
        }
    }
}
