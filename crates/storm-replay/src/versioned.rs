//! "Versioned" decoder — faithful port of `VersionedDecoder` (heroprotocol/decoders.py, MIT).
//! The format is entirely byte-aligned → a byte cursor is enough.
//! Tags: 0 array, 1 bitblob, 2 blob, 3 choice, 4 optional, 5 struct, 6 u8, 7 u32, 8 u64, 9 vint.

use crate::error::{Error, Result};
use crate::typeinfo::TypeInfo;
use crate::value::Value;

/// Maximum nesting of `skip_instance` (unknown fields of a newer build). The skip is driven by
/// the bytes alone, so without a bound a crafted stream nests it until the stack overflows
/// (process abort); real replays nest a handful of levels.
const MAX_SKIP_DEPTH: u32 = 64;

pub struct VersionedDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    typeinfos: &'a [TypeInfo],
}

impl<'a> VersionedDecoder<'a> {
    pub fn new(data: &'a [u8], typeinfos: &'a [TypeInfo]) -> Self {
        VersionedDecoder {
            data,
            pos: 0,
            typeinfos,
        }
    }

    pub fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    pub fn used_bytes(&self) -> usize {
        self.pos
    }

    /// Bytes not consumed yet. Every array item / struct field takes at least one byte, so this
    /// bounds any preallocation sized by a count read from the file (a crafted count would
    /// otherwise make `Vec::with_capacity` abort the whole process on allocation failure).
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Result<u8> {
        let b = *self
            .data
            .get(self.pos)
            .ok_or_else(|| Error::Truncated(format!("byte {} (versioned)", self.pos)))?;
        self.pos += 1;
        Ok(b)
    }

    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| Error::Truncated(format!("{n} bytes at {} (versioned)", self.pos)))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn expect_skip(&mut self, expected: u8) -> Result<()> {
        let got = self.read_u8()?;
        if got != expected {
            return Err(Error::Corrupted(format!(
                "tag {got} ≠ {expected} at byte {} (versioned)",
                self.pos - 1
            )));
        }
        Ok(())
    }

    /// The reference's varint: bit 0 of the 1st byte = sign, then 7 bits/byte (bit 7 = continuation).
    fn vint(&mut self) -> Result<i64> {
        let b = i64::from(self.read_u8()?);
        let negative = b & 1 != 0;
        let mut result = (b >> 1) & 0x3F;
        let mut bits = 6;
        let mut cont = b & 0x80 != 0;
        while cont {
            let b = i64::from(self.read_u8()?);
            result |= (b & 0x7F) << bits;
            bits += 7;
            cont = b & 0x80 != 0;
        }
        Ok(if negative { -result } else { result })
    }

    fn typeinfo(&self, typeid: usize) -> Result<&'a TypeInfo> {
        self.typeinfos
            .get(typeid)
            .ok_or_else(|| Error::Protocol(format!("typeid {typeid} outside the table")))
    }

    /// Gameloop delta (`NNet.SVarUint32` = choice of `_int`) without materialising a `Value`
    /// — hot path: one call per event of each stream.
    pub fn svaruint32_value(&mut self, typeid: usize) -> Result<i64> {
        if let TypeInfo::Choice { fields, .. } = self.typeinfo(typeid)? {
            self.expect_skip(3)?;
            let tag = self.vint()?;
            if let Some(&(_, chosen)) = fields.get(&tag).map(|(n, t)| (n, *t)).as_ref() {
                if matches!(self.typeinfo(chosen)?, TypeInfo::Int { .. }) {
                    self.expect_skip(9)?;
                    return self.vint();
                }
                return self
                    .instance(chosen)?
                    .as_int()
                    .ok_or_else(|| Error::Corrupted("svaruint32 non entier".into()));
            }
            self.skip_instance(0)?;
            return Err(Error::Corrupted(format!("svaruint32: unknown tag {tag}")));
        }
        self.instance(typeid)?
            .first_field_int()
            .ok_or_else(|| Error::Corrupted("svaruint32 invalide".into()))
    }

    pub fn instance(&mut self, typeid: usize) -> Result<Value> {
        Ok(match self.typeinfo(typeid)? {
            TypeInfo::Int { .. } => {
                self.expect_skip(9)?;
                Value::Int(self.vint()?)
            }
            TypeInfo::Blob { .. } => {
                self.expect_skip(2)?;
                let len = usize::try_from(self.vint()?)
                    .map_err(|_| Error::Corrupted("negative blob length".into()))?;
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
                Value::Real(f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))
            }
            TypeInfo::Real64 => {
                self.expect_skip(8)?;
                let b: [u8; 8] = self
                    .read_bytes(8)?
                    .try_into()
                    .map_err(|_| Error::Truncated("real64".into()))?;
                Value::Real(f64::from_be_bytes(b))
            }
            TypeInfo::Null => Value::Null,
            TypeInfo::Array { typeid, .. } => {
                self.expect_skip(0)?;
                let len = usize::try_from(self.vint()?)
                    .map_err(|_| Error::Corrupted("negative array length".into()))?;
                let typeid = *typeid;
                let mut items = Vec::with_capacity(len.min(self.remaining()));
                for _ in 0..len {
                    items.push(self.instance(typeid)?);
                }
                Value::Array(items)
            }
            TypeInfo::BitArray { .. } => {
                self.expect_skip(1)?;
                let bits = u64::try_from(self.vint()?)
                    .map_err(|_| Error::Corrupted("negative bitarray length".into()))?;
                let data = self.read_bytes(bits.div_ceil(8) as usize)?.to_vec();
                Value::BitArrayBytes { bits, data }
            }
            TypeInfo::Optional { typeid } => {
                self.expect_skip(4)?;
                let typeid = *typeid;
                if self.read_u8()? != 0 {
                    self.instance(typeid)?
                } else {
                    Value::Null
                }
            }
            TypeInfo::Choice { fields, .. } => {
                self.expect_skip(3)?;
                let tag = self.vint()?;
                match fields.get(&tag) {
                    Some((name, typeid)) => {
                        let (name, typeid) = (name.clone(), *typeid);
                        let v = self.instance(typeid)?;
                        Value::Struct(vec![(name, v)])
                    }
                    None => {
                        // reference: unknown tag → skip + empty struct (build compatibility)
                        self.skip_instance(0)?;
                        Value::Struct(Vec::new())
                    }
                }
            }
            TypeInfo::Struct { fields } => {
                self.expect_skip(5)?;
                let count = usize::try_from(self.vint()?)
                    .map_err(|_| Error::Corrupted("negative struct size".into()))?;
                let mut result: Vec<(std::sync::Arc<str>, Value)> =
                    Vec::with_capacity(count.min(self.remaining()));
                for _ in 0..count {
                    let tag = self.vint()?;
                    match fields.iter().find(|f| f.2 == tag) {
                        Some((name, ftypeid, _)) => {
                            let (name, ftypeid) = (name.clone(), *ftypeid);
                            let v = self.instance(ftypeid)?;
                            if name.as_ref() == "__parent" {
                                match v {
                                    Value::Struct(parent) => result.extend(parent),
                                    other if fields.len() == 1 => return Ok(other),
                                    other => result.push((name, other)),
                                }
                            } else {
                                result.push((name, v));
                            }
                        }
                        None => self.skip_instance(0)?, // unknown field (build compatibility)
                    }
                }
                Value::Struct(result)
            }
        })
    }

    fn skip_instance(&mut self, depth: u32) -> Result<()> {
        if depth >= MAX_SKIP_DEPTH {
            return Err(Error::Corrupted(format!(
                "skip: nesting deeper than {MAX_SKIP_DEPTH} at byte {} (versioned)",
                self.pos
            )));
        }
        let depth = depth + 1;
        let skip = self.read_u8()?;
        match skip {
            0 => {
                let len = self.vint()?;
                for _ in 0..len {
                    self.skip_instance(depth)?;
                }
            }
            1 => {
                let bits = u64::try_from(self.vint()?)
                    .map_err(|_| Error::Corrupted("skip bitblob".into()))?;
                self.read_bytes(bits.div_ceil(8) as usize)?;
            }
            2 => {
                let len = usize::try_from(self.vint()?)
                    .map_err(|_| Error::Corrupted("skip blob".into()))?;
                self.read_bytes(len)?;
            }
            3 => {
                self.vint()?;
                self.skip_instance(depth)?;
            }
            4 => {
                if self.read_u8()? != 0 {
                    self.skip_instance(depth)?;
                }
            }
            5 => {
                let len = self.vint()?;
                for _ in 0..len {
                    self.vint()?;
                    self.skip_instance(depth)?;
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
            other => {
                return Err(Error::Corrupted(format!(
                    "skip: unknown tag {other} at byte {} (versioned)",
                    self.pos - 1
                )))
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Reference vint: encodings computed by hand from decoders.py (_vint).
    /// 0 → [0x00]; 1 → [0x02]; -1 → [0x03]; 63 → [0x7E]; 64 → [0x80, 0x02].
    #[test]
    fn vint_reference() {
        let cases: &[(&[u8], i64)] = &[
            (&[0x00], 0),
            (&[0x02], 1),
            (&[0x03], -1),
            (&[0x7E], 63),
            (&[0x80, 0x01], 64),
            (&[0x81, 0x01], -64),
        ];
        for (bytes, expected) in cases {
            let mut d = VersionedDecoder::new(bytes, &[]);
            assert_eq!(d.vint().unwrap(), *expected, "bytes {bytes:?}");
        }
    }

    /// Security review C1: a struct field count of 2^35 read from the header used to reach
    /// `Vec::with_capacity` as is — allocation failure, process abort. Now a plain error.
    #[test]
    fn huge_struct_count_is_an_error_not_an_abort() {
        let proto = crate::protocol::latest_protocol().unwrap();
        let c1 = [0x05, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02];
        assert!(proto.decode_header(&c1).is_err());
        assert!(proto.decode_details(&c1).is_err());
        // same for an array length (struct with one field holding a huge array)
        let mut d = VersionedDecoder::new(&[0x00, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02], &[]);
        assert!(d.instance(0).is_err(), "typeid outside an empty table");
    }

    /// Unknown fields are skipped by reading the bytes alone: a crafted nesting is bounded
    /// instead of overflowing the stack.
    #[test]
    fn deep_skip_nesting_is_an_error_not_a_stack_overflow() {
        let proto = crate::protocol::latest_protocol().unwrap();
        // struct, 1 field, unknown tag 63, then 1M nested "optional present" skips
        let mut bytes = vec![0x05, 0x02, 0x7E];
        for _ in 0..1_000_000 {
            bytes.extend_from_slice(&[0x04, 0x01]);
        }
        bytes.push(0x06);
        bytes.push(0x00);
        let e = proto.decode_header(&bytes).unwrap_err();
        assert!(e.to_string().contains("nesting"), "{e}");
        // a shallow unknown field is still skipped (build compatibility)
        let ok = [0x05, 0x02, 0x7E, 0x04, 0x01, 0x04, 0x01, 0x06, 0x00];
        assert!(proto.decode_header(&ok).is_ok());
    }
}
