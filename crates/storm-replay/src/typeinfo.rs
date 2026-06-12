//! Grammaire des protocoles : les `typeinfos` exportées de heroprotocol, pré-parsées.
//! Les bounds (`lo`, `bits`) sont significatifs pour le décodeur bitpacked ;
//! le décodeur versioned les ignore (l'encodage versioned est auto-décrit).

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

/// Hasher trivial pour clés `i64` (tags de choice, eventids) — SipHash coûte cher sur le
/// chemin chaud (~10⁶ lookups/replay), un multiply-shift suffit pour des petits entiers.
#[derive(Default)]
pub struct I64Hasher(u64);

impl Hasher for I64Hasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_i64(&mut self, v: i64) {
        self.0 = (v as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = v.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub type FastI64Map<V> = HashMap<i64, V, BuildHasherDefault<I64Hasher>>;

#[derive(Debug)]
pub enum TypeInfo {
    Int { lo: i64, bits: u32 },
    Blob { lo: i64, bits: u32 },
    Bool,
    Fourcc,
    Real32,
    Real64,
    Null,
    Array { lo: i64, bits: u32, typeid: usize },
    BitArray { lo: i64, bits: u32 },
    Optional { typeid: usize },
    Choice { lo: i64, bits: u32, fields: FastI64Map<(Arc<str>, usize)> },
    Struct { fields: Vec<(Arc<str>, usize, i64)> },
}

fn err(msg: impl Into<String>) -> Error {
    Error::Protocol(msg.into())
}

fn bounds(args: &[serde_json::Value], i: usize) -> Result<(i64, u32)> {
    let b = args
        .first()
        .and_then(|v| v.as_array())
        .ok_or_else(|| err(format!("typeinfo {i}: bounds absents")))?;
    Ok((
        b.first().and_then(|v| v.as_i64()).ok_or_else(|| err(format!("typeinfo {i}: lo")))?,
        b.get(1).and_then(|v| v.as_u64()).ok_or_else(|| err(format!("typeinfo {i}: bits")))?
            as u32,
    ))
}

pub fn parse_typeinfos(json: &serde_json::Value) -> Result<Vec<TypeInfo>> {
    let list = json.as_array().ok_or_else(|| err("typeinfos : pas un tableau"))?;
    let mut out = Vec::with_capacity(list.len());
    for (i, entry) in list.iter().enumerate() {
        let pair = entry.as_array().ok_or_else(|| err(format!("typeinfo {i} invalide")))?;
        let method = pair
            .first()
            .and_then(|v| v.as_str())
            .ok_or_else(|| err(format!("typeinfo {i}: méthode")))?;
        let args = pair
            .get(1)
            .and_then(|v| v.as_array())
            .ok_or_else(|| err(format!("typeinfo {i}: args")))?;
        let ti = match method {
            "_bool" => TypeInfo::Bool,
            "_fourcc" => TypeInfo::Fourcc,
            "_real32" => TypeInfo::Real32,
            "_real64" => TypeInfo::Real64,
            "_null" => TypeInfo::Null,
            "_int" => {
                let (lo, bits) = bounds(args, i)?;
                TypeInfo::Int { lo, bits }
            }
            "_blob" => {
                let (lo, bits) = bounds(args, i)?;
                TypeInfo::Blob { lo, bits }
            }
            "_bitarray" => {
                let (lo, bits) = bounds(args, i)?;
                TypeInfo::BitArray { lo, bits }
            }
            "_array" => {
                let (lo, bits) = bounds(args, i)?;
                TypeInfo::Array {
                    lo,
                    bits,
                    typeid: args
                        .get(1)
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| err(format!("array {i}: typeid")))?
                        as usize,
                }
            }
            "_optional" => TypeInfo::Optional {
                typeid: args
                    .first()
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| err(format!("optional {i}: typeid")))?
                    as usize,
            },
            "_choice" => {
                let (lo, bits) = bounds(args, i)?;
                let map = args
                    .get(1)
                    .and_then(|v| v.as_object())
                    .ok_or_else(|| err(format!("choice {i}: fields")))?;
                let mut fields =
                    FastI64Map::with_capacity_and_hasher(map.len(), Default::default());
                for (tag, field) in map {
                    let f = field
                        .as_array()
                        .ok_or_else(|| err(format!("choice {i}: field")))?;
                    fields.insert(
                        tag.parse::<i64>().map_err(|_| err(format!("choice {i}: tag {tag}")))?,
                        (
                            f.first()
                                .and_then(|v| v.as_str())
                                .ok_or_else(|| err(format!("choice {i}: nom")))?
                                .into(),
                            f.get(1)
                                .and_then(|v| v.as_u64())
                                .ok_or_else(|| err(format!("choice {i}: typeid")))?
                                as usize,
                        ),
                    );
                }
                TypeInfo::Choice { lo, bits, fields }
            }
            "_struct" => {
                let list = args
                    .first()
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| err(format!("struct {i}: fields")))?;
                let mut fields = Vec::with_capacity(list.len());
                for field in list {
                    let f = field
                        .as_array()
                        .ok_or_else(|| err(format!("struct {i}: field")))?;
                    fields.push((
                        f.first()
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| err(format!("struct {i}: nom")))?
                            .into(),
                        f.get(1)
                            .and_then(|v| v.as_u64())
                            .ok_or_else(|| err(format!("struct {i}: typeid")))?
                            as usize,
                        f.get(2)
                            .and_then(|v| v.as_i64())
                            .ok_or_else(|| err(format!("struct {i}: tag")))?,
                    ));
                }
                TypeInfo::Struct { fields }
            }
            other => return Err(err(format!("typeinfo {i}: méthode inconnue {other}"))),
        };
        out.push(ti);
    }
    Ok(out)
}
