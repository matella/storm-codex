//! Valeurs décodées — équivalent des structures Python produites par heroprotocol.

/// Valeur décodée d'un stream de replay.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Bool(bool),
    Blob(Vec<u8>),
    Fourcc([u8; 4]),
    Real(f64),
    Array(Vec<Value>),
    /// `_bitarray` du décodeur **versioned** : octets alignés (référence : `(longueur, bytes)`).
    BitArrayBytes { bits: u64, data: Vec<u8> },
    /// `_bitarray` du décodeur **bitpacked** : entier de `bits` bits (référence :
    /// `(longueur, int)`). Stocké en octets big-endian alignés à droite car il peut
    /// dépasser 64 bits (longueur encodée sur 8 bits → jusqu'à 255 bits).
    BitArrayInt { bits: u64, value: Vec<u8> },
    /// Structs **et** choices (un choice = struct à un seul champ, comme en Python).
    /// L'ordre d'insertion est préservé.
    Struct(Vec<(String, Value)>),
}

impl Value {
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Struct(fields) => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
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

    /// Blobs texte (noms, titres…) : les replays HotS sont en UTF-8.
    pub fn as_str_lossy(&self) -> Option<String> {
        self.as_blob().map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Valeur du premier champ d'un struct/choice (sémantique `_varuint32_value`).
    pub fn first_field_int(&self) -> Option<i64> {
        match self {
            Value::Struct(fields) => fields.first().and_then(|(_, v)| v.as_int()),
            _ => None,
        }
    }
}
