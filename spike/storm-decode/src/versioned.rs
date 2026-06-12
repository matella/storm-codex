//! Port fidèle du `VersionedDecoder` de heroprotocol/decoders.py (référence Blizzard, MIT).
//! Le format « versioned » est intégralement aligné sur l'octet (tous les accès passent par
//! read_bits(8) ou read_aligned_bytes dans la référence) → un simple curseur d'octets suffit.

use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;

/// Équivalent des valeurs Python produites par le décodeur de référence.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Bool(bool),
    Blob(Vec<u8>),
    Fourcc([u8; 4]),
    Real(f64),
    Array(Vec<Value>),
    BitArray { bits: u64, data: Vec<u8> },
    /// Structs ET choices (un choice = struct à un seul champ, comme en Python).
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

    /// Blobs texte (noms, titres) : UTF-8 avec remplacement (les replays sont UTF-8).
    pub fn as_str_lossy(&self) -> Option<String> {
        match self {
            Value::Blob(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }

    /// Premier champ d'un struct/choice (sémantique `_varuint32_value` de la référence).
    pub fn first_field_int(&self) -> Option<i64> {
        match self {
            Value::Struct(fields) => fields.first().and_then(|(_, v)| v.as_int()),
            _ => None,
        }
    }
}

/// `typeinfos` pré-parsées depuis le JSON exporté de heroprotocol.
#[derive(Debug)]
pub enum TypeInfo {
    Int,
    Blob,
    Bool,
    Fourcc,
    Real32,
    Real64,
    Null,
    Array { typeid: usize },
    BitArray,
    Optional { typeid: usize },
    Choice { fields: HashMap<i64, (String, usize)> },
    Struct { fields: Vec<(String, usize, i64)> },
}

pub fn parse_typeinfos(json: &serde_json::Value) -> Result<Vec<TypeInfo>> {
    let list = json.as_array().ok_or_else(|| anyhow!("typeinfos: pas un tableau"))?;
    let mut out = Vec::with_capacity(list.len());
    for (i, entry) in list.iter().enumerate() {
        let pair = entry.as_array().ok_or_else(|| anyhow!("typeinfo {i} invalide"))?;
        let method = pair[0].as_str().ok_or_else(|| anyhow!("typeinfo {i}: méthode"))?;
        let args = pair[1].as_array().ok_or_else(|| anyhow!("typeinfo {i}: args"))?;
        let ti = match method {
            "_int" => TypeInfo::Int,
            "_blob" => TypeInfo::Blob,
            "_bool" => TypeInfo::Bool,
            "_fourcc" => TypeInfo::Fourcc,
            "_real32" => TypeInfo::Real32,
            "_real64" => TypeInfo::Real64,
            "_null" => TypeInfo::Null,
            "_bitarray" => TypeInfo::BitArray,
            "_array" => TypeInfo::Array {
                typeid: args[1].as_u64().ok_or_else(|| anyhow!("array {i}: typeid"))? as usize,
            },
            "_optional" => TypeInfo::Optional {
                typeid: args[0].as_u64().ok_or_else(|| anyhow!("optional {i}: typeid"))? as usize,
            },
            "_choice" => {
                let map = args[1].as_object().ok_or_else(|| anyhow!("choice {i}: fields"))?;
                let mut fields = HashMap::with_capacity(map.len());
                for (tag, field) in map {
                    let f = field.as_array().ok_or_else(|| anyhow!("choice {i}: field"))?;
                    fields.insert(
                        tag.parse::<i64>()?,
                        (
                            f[0].as_str().ok_or_else(|| anyhow!("choice {i}: nom"))?.to_owned(),
                            f[1].as_u64().ok_or_else(|| anyhow!("choice {i}: typeid"))? as usize,
                        ),
                    );
                }
                TypeInfo::Choice { fields }
            }
            "_struct" => {
                let list = args[0].as_array().ok_or_else(|| anyhow!("struct {i}: fields"))?;
                let mut fields = Vec::with_capacity(list.len());
                for field in list {
                    let f = field.as_array().ok_or_else(|| anyhow!("struct {i}: field"))?;
                    fields.push((
                        f[0].as_str().ok_or_else(|| anyhow!("struct {i}: nom"))?.to_owned(),
                        f[1].as_u64().ok_or_else(|| anyhow!("struct {i}: typeid"))? as usize,
                        f[2].as_i64().ok_or_else(|| anyhow!("struct {i}: tag"))?,
                    ));
                }
                TypeInfo::Struct { fields }
            }
            other => bail!("typeinfo {i}: méthode inconnue {other}"),
        };
        out.push(ti);
    }
    Ok(out)
}

pub struct VersionedDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    typeinfos: &'a [TypeInfo],
}

impl<'a> VersionedDecoder<'a> {
    pub fn new(data: &'a [u8], typeinfos: &'a [TypeInfo]) -> Self {
        VersionedDecoder { data, pos: 0, typeinfos }
    }

    pub fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn used_bytes(&self) -> usize {
        self.pos
    }

    fn read_u8(&mut self) -> Result<u8> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| anyhow!("tronqué à l'octet {}", self.pos))?;
        self.pos += 1;
        Ok(b)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos + n;
        if end > self.data.len() {
            bail!("tronqué : {n} octets demandés à {}", self.pos);
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn expect_skip(&mut self, expected: u8) -> Result<()> {
        let got = self.read_u8()?;
        if got != expected {
            bail!("corrompu : tag {got} ≠ {expected} à l'octet {}", self.pos - 1);
        }
        Ok(())
    }

    /// Varint zigzag de la référence : bit 0 du 1er octet = signe, puis 7 bits/octet.
    fn vint(&mut self) -> Result<i64> {
        let b = self.read_u8()? as i64;
        let negative = b & 1 != 0;
        let mut result = (b >> 1) & 0x3F;
        let mut bits = 6;
        let mut cont = b & 0x80 != 0;
        while cont {
            let b = self.read_u8()? as i64;
            result |= (b & 0x7F) << bits;
            bits += 7;
            cont = b & 0x80 != 0;
        }
        Ok(if negative { -result } else { result })
    }

    pub fn instance(&mut self, typeid: usize) -> Result<Value> {
        let ti = self
            .typeinfos
            .get(typeid)
            .ok_or_else(|| anyhow!("typeid {typeid} hors table"))?;
        Ok(match ti {
            TypeInfo::Int => {
                self.expect_skip(9)?;
                Value::Int(self.vint()?)
            }
            TypeInfo::Blob => {
                self.expect_skip(2)?;
                let len = self.vint()? as usize;
                Value::Blob(self.read_bytes(len)?.to_vec())
            }
            TypeInfo::Bool => {
                self.expect_skip(6)?;
                Value::Bool(self.read_u8()? != 0)
            }
            TypeInfo::Fourcc => {
                self.expect_skip(7)?;
                let b = self.read_bytes(4)?;
                Value::Fourcc([b[0], b[1], b[2], b[3]])
            }
            TypeInfo::Real32 => {
                self.expect_skip(7)?;
                let b = self.read_bytes(4)?;
                Value::Real(f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64)
            }
            TypeInfo::Real64 => {
                self.expect_skip(8)?;
                let b = self.read_bytes(8)?;
                Value::Real(f64::from_be_bytes(b.try_into().expect("8 octets")))
            }
            TypeInfo::Null => Value::Null,
            TypeInfo::Array { typeid } => {
                self.expect_skip(0)?;
                let len = self.vint()? as usize;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    items.push(self.instance(*typeid)?);
                }
                Value::Array(items)
            }
            TypeInfo::BitArray => {
                self.expect_skip(1)?;
                let bits = self.vint()? as u64;
                let data = self.read_bytes(bits.div_ceil(8) as usize)?.to_vec();
                Value::BitArray { bits, data }
            }
            TypeInfo::Optional { typeid } => {
                self.expect_skip(4)?;
                if self.read_u8()? != 0 {
                    self.instance(*typeid)?
                } else {
                    Value::Null
                }
            }
            TypeInfo::Choice { fields } => {
                self.expect_skip(3)?;
                let tag = self.vint()?;
                match fields.get(&tag) {
                    Some((name, typeid)) => {
                        let v = self.instance(*typeid)?;
                        Value::Struct(vec![(name.clone(), v)])
                    }
                    None => {
                        // comportement référence : champ inconnu → skip + struct vide
                        self.skip_instance()?;
                        Value::Struct(Vec::new())
                    }
                }
            }
            TypeInfo::Struct { fields } => {
                self.expect_skip(5)?;
                let count = self.vint()? as usize;
                let mut result: Vec<(String, Value)> = Vec::with_capacity(count);
                for _ in 0..count {
                    let tag = self.vint()?;
                    match fields.iter().find(|f| f.2 == tag) {
                        Some((name, typeid, _)) => {
                            let v = self.instance(*typeid)?;
                            if name == "__parent" {
                                match v {
                                    Value::Struct(parent) => result.extend(parent),
                                    other if fields.len() == 1 => return Ok(other),
                                    other => result.push((name.clone(), other)),
                                }
                            } else {
                                result.push((name.clone(), v));
                            }
                        }
                        None => self.skip_instance()?, // champ inconnu (compat builds)
                    }
                }
                Value::Struct(result)
            }
        })
    }

    fn skip_instance(&mut self) -> Result<()> {
        let skip = self.read_u8()?;
        match skip {
            0 => {
                // array
                let len = self.vint()?;
                for _ in 0..len {
                    self.skip_instance()?;
                }
            }
            1 => {
                // bitblob
                let bits = self.vint()? as u64;
                self.read_bytes(bits.div_ceil(8) as usize)?;
            }
            2 => {
                // blob
                let len = self.vint()? as usize;
                self.read_bytes(len)?;
            }
            3 => {
                // choice
                self.vint()?;
                self.skip_instance()?;
            }
            4 => {
                // optional
                if self.read_u8()? != 0 {
                    self.skip_instance()?;
                }
            }
            5 => {
                // struct
                let len = self.vint()?;
                for _ in 0..len {
                    self.vint()?;
                    self.skip_instance()?;
                }
            }
            6 => {
                self.read_bytes(1)?;
            }
            7 => {
                self.read_bytes(4)?;
            }
            8 => {
                self.read_bytes(8)?;
            }
            9 => {
                self.vint()?;
            }
            other => bail!("skip : tag inconnu {other} à l'octet {}", self.pos - 1),
        }
        Ok(())
    }
}
