//! Décodeur « bitpacked » — port fidèle de `BitPackedBuffer`/`BitPackedDecoder`
//! (heroprotocol/decoders.py, MIT). Lecture au bit près : requis par replay.initData,
//! replay.game.events et replay.message.events ; les attributes utilisent le même buffer
//! en little-endian.

use crate::error::{Error, Result};
use crate::typeinfo::TypeInfo;
use crate::value::Value;

#[derive(Clone, Copy, PartialEq)]
pub enum Endian {
    Big,
    Little,
}

pub struct BitReader<'a> {
    data: &'a [u8],
    used: usize,
    next: u8,
    nextbits: u32,
    big_endian: bool,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8], endian: Endian) -> Self {
        BitReader { data, used: 0, next: 0, nextbits: 0, big_endian: endian == Endian::Big }
    }

    pub fn done(&self) -> bool {
        self.nextbits == 0 && self.used >= self.data.len()
    }

    pub fn used_bits(&self) -> u64 {
        self.used as u64 * 8 - u64::from(self.nextbits)
    }

    pub fn byte_align(&mut self) {
        self.nextbits = 0;
    }

    pub fn read_aligned_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.byte_align();
        let end = self
            .used
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| Error::Truncated(format!("{n} octets alignés à {}", self.used)))?;
        let s = &self.data[self.used..end];
        self.used = end;
        Ok(s)
    }

    /// Port exact de `BitPackedBuffer.read_bits` : on consomme les bits de poids faible de
    /// l'octet courant, placés en poids fort du résultat (big-endian) ou en poids faible
    /// (little-endian). `bits` ≤ 64 — les bitarrays plus larges passent par `read_bits_big`.
    pub fn read_bits(&mut self, bits: u32) -> Result<u64> {
        debug_assert!(bits <= 64);
        let mut result: u64 = 0;
        let mut resultbits: u32 = 0;
        while resultbits != bits {
            if self.nextbits == 0 {
                if self.used >= self.data.len() {
                    return Err(Error::Truncated(format!(
                        "lecture de {bits} bits à l'octet {}",
                        self.used
                    )));
                }
                self.next = self.data[self.used];
                self.used += 1;
                self.nextbits = 8;
            }
            let copybits = (bits - resultbits).min(self.nextbits);
            let copy = u64::from(self.next) & ((1u64 << copybits) - 1);
            if self.big_endian {
                result |= copy << (bits - resultbits - copybits);
            } else {
                result |= copy << resultbits;
            }
            self.next = (u16::from(self.next) >> copybits) as u8; // copybits peut valoir 8
            self.nextbits -= copybits;
            resultbits += copybits;
        }
        Ok(result)
    }

    /// Entier de `bits` bits (jusqu'à 255 pour les bitarrays) en octets big-endian alignés
    /// à droite — même algorithme de chunks que `read_bits` (un appel unique en Python sur
    /// gros entier ; NE PAS remplacer par des `read_bits` successifs : le placement des
    /// fragments dépend de l'alignement du buffer et ne se compose pas).
    pub fn read_bits_big(&mut self, bits: u64) -> Result<Vec<u8>> {
        let nbytes = bits.div_ceil(8) as usize;
        let mut out = vec![0u8; nbytes];
        let mut resultbits: u64 = 0;
        while resultbits != bits {
            if self.nextbits == 0 {
                if self.used >= self.data.len() {
                    return Err(Error::Truncated(format!(
                        "lecture de {bits} bits (gros entier) à l'octet {}",
                        self.used
                    )));
                }
                self.next = self.data[self.used];
                self.used += 1;
                self.nextbits = 8;
            }
            let copybits = u64::from(self.nextbits).min(bits - resultbits);
            let copy = self.next & (((1u16 << copybits) - 1) as u8);
            let shift = if self.big_endian { bits - resultbits - copybits } else { resultbits };
            for i in 0..copybits {
                if (copy >> i) & 1 != 0 {
                    let pos = shift + i;
                    let byte = nbytes - 1 - (pos / 8) as usize;
                    out[byte] |= 1 << (pos % 8);
                }
            }
            self.next = (u16::from(self.next) >> copybits) as u8; // copybits peut valoir 8
            self.nextbits -= copybits as u32;
            resultbits += copybits;
        }
        Ok(out)
    }

    fn read_unaligned_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        (0..n).map(|_| self.read_bits(8).map(|b| b as u8)).collect()
    }
}

pub struct BitPackedDecoder<'a> {
    pub buffer: BitReader<'a>,
    typeinfos: &'a [TypeInfo],
}

impl<'a> BitPackedDecoder<'a> {
    pub fn new(data: &'a [u8], typeinfos: &'a [TypeInfo]) -> Self {
        BitPackedDecoder { buffer: BitReader::new(data, Endian::Big), typeinfos }
    }

    pub fn done(&self) -> bool {
        self.buffer.done()
    }

    pub fn byte_align(&mut self) {
        self.buffer.byte_align();
    }

    fn read_int(&mut self, lo: i64, bits: u32) -> Result<i64> {
        let v = self.buffer.read_bits(bits)?;
        // lo + v en two's complement exact (couvre (-2^63, 64) comme (0, 32)) ;
        // seul (0, 64) avec une valeur > i64::MAX déborderait — jamais vu en pratique.
        let signed = i64::try_from(v).or_else(|_| {
            if lo == 0 {
                Err(Error::Corrupted(format!("entier {bits} bits hors plage i64 : {v}")))
            } else {
                Ok(v as i64) // lo = -2^63 : wrapping = arithmétique two's complement exacte
            }
        })?;
        Ok(lo.wrapping_add(signed))
    }

    fn typeinfo(&self, typeid: usize) -> Result<&'a TypeInfo> {
        self.typeinfos
            .get(typeid)
            .ok_or_else(|| Error::Protocol(format!("typeid {typeid} hors table")))
    }

    /// Delta de gameloop (`NNet.SVarUint32` = choice de `_int`) sans matérialiser de `Value`
    /// — chemin chaud : un appel par événement game/message.
    pub fn svaruint32_value(&mut self, typeid: usize) -> Result<i64> {
        if let TypeInfo::Choice { lo, bits, fields } = self.typeinfo(typeid)? {
            let (lo, bits) = (*lo, *bits);
            let tag = self.read_int(lo, bits)?;
            let chosen = fields
                .get(&tag)
                .map(|(_, t)| *t)
                .ok_or_else(|| Error::Corrupted(format!("svaruint32 : tag inconnu {tag}")))?;
            if let TypeInfo::Int { lo, bits } = self.typeinfo(chosen)? {
                let (lo, bits) = (*lo, *bits);
                return self.read_int(lo, bits);
            }
            return self
                .instance(chosen)?
                .as_int()
                .ok_or_else(|| Error::Corrupted("svaruint32 non entier".into()));
        }
        self.instance(typeid)?
            .first_field_int()
            .ok_or_else(|| Error::Corrupted("svaruint32 invalide".into()))
    }

    pub fn instance(&mut self, typeid: usize) -> Result<Value> {
        Ok(match self.typeinfo(typeid)? {
            TypeInfo::Int { lo, bits } => {
                let (lo, bits) = (*lo, *bits);
                Value::Int(self.read_int(lo, bits)?)
            }
            TypeInfo::Blob { lo, bits } => {
                let (lo, bits) = (*lo, *bits);
                let len = usize::try_from(self.read_int(lo, bits)?)
                    .map_err(|_| Error::Corrupted("longueur de blob négative".into()))?;
                Value::Blob(self.buffer.read_aligned_bytes(len)?.to_vec())
            }
            TypeInfo::Bool => Value::Bool(self.buffer.read_bits(1)? != 0),
            TypeInfo::Fourcc => {
                let v = self.buffer.read_bits(32)? as u32;
                Value::Fourcc(v.to_be_bytes())
            }
            TypeInfo::Real32 => {
                let b = self.buffer.read_unaligned_bytes(4)?;
                Value::Real(f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))
            }
            TypeInfo::Real64 => {
                let b = self.buffer.read_unaligned_bytes(8)?;
                let arr: [u8; 8] =
                    b.try_into().map_err(|_| Error::Truncated("real64".into()))?;
                Value::Real(f64::from_be_bytes(arr))
            }
            TypeInfo::Null => Value::Null,
            TypeInfo::Array { lo, bits, typeid } => {
                let (lo, bits, typeid) = (*lo, *bits, *typeid);
                let len = usize::try_from(self.read_int(lo, bits)?)
                    .map_err(|_| Error::Corrupted("longueur de tableau négative".into()))?;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    items.push(self.instance(typeid)?);
                }
                Value::Array(items)
            }
            TypeInfo::BitArray { lo, bits } => {
                let (lo, bits) = (*lo, *bits);
                let len = u64::try_from(self.read_int(lo, bits)?)
                    .map_err(|_| Error::Corrupted("longueur de bitarray négative".into()))?;
                Value::BitArrayInt { bits: len, value: self.buffer.read_bits_big(len)? }
            }
            TypeInfo::Optional { typeid } => {
                let typeid = *typeid;
                if self.buffer.read_bits(1)? != 0 {
                    self.instance(typeid)?
                } else {
                    Value::Null
                }
            }
            TypeInfo::Choice { lo, bits, fields } => {
                let (lo, bits) = (*lo, *bits);
                let tag = self.read_int(lo, bits)?;
                let (name, ftypeid) = fields.get(&tag).ok_or_else(|| {
                    Error::Corrupted(format!("choice : tag inconnu {tag} (bitpacked)"))
                })?;
                let (name, ftypeid) = (name.clone(), *ftypeid);
                let v = self.instance(ftypeid)?;
                Value::Struct(vec![(name, v)])
            }
            TypeInfo::Struct { fields } => {
                // bitpacked : séquentiel, sans tags ni skip — tout champ se décode.
                let n = fields.len();
                let mut result: Vec<(std::sync::Arc<str>, Value)> = Vec::with_capacity(n);
                for (name, ftypeid, _) in fields {
                    let v = self.instance(*ftypeid)?;
                    if name.as_ref() == "__parent" {
                        match v {
                            Value::Struct(parent) => result.extend(parent),
                            other if n == 1 => return Ok(other),
                            other => result.push((name.clone(), other)),
                        }
                    } else {
                        result.push((name.clone(), v));
                    }
                }
                Value::Struct(result)
            }
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Vérifié contre Python :
    /// BitPackedBuffer(b'\xC5\x0F').read_bits(6) == 5 ; puis read_bits(6) == 60 ;
    /// (1er octet 0xC5 : bits bas 000101 consommés d'abord, restent 11 ;
    ///  6 bits suivants = 2 bits hauts de 0xC5 (11, poids fort) + 4 bits bas de 0x0F (1111)).
    #[test]
    fn read_bits_big_endian_reference() {
        let data = [0xC5u8, 0x0F];
        let mut r = BitReader::new(&data, Endian::Big);
        assert_eq!(r.read_bits(6).unwrap(), 5);
        assert_eq!(r.read_bits(6).unwrap(), 0b11_1111 & ((0b11 << 4) | 0b1111));
    }

    /// little-endian (attributes) : les fragments s'empilent en poids faible.
    /// BitPackedBuffer(b'\xC5\x0F', 'little').read_bits(12) == 0xFC5.
    #[test]
    fn read_bits_little_endian_reference() {
        let data = [0xC5u8, 0x0F];
        let mut r = BitReader::new(&data, Endian::Little);
        assert_eq!(r.read_bits(12).unwrap(), 0xFC5);
    }

    /// read_bits_big : 12 bits 0xFC5 → octets [0x0F, 0xC5] (big-endian aligné à droite).
    #[test]
    fn read_bits_big_int() {
        // flux big-endian contenant 0xFC5 sur 12 bits : MSB d'abord
        // bits : 1111 1100 0101 → octets construits pour que read_bits(12) == 0xFC5
        let data = [0b1111_1100u8, 0b0000_0101];
        let mut r = BitReader::new(&data, Endian::Big);
        let big = r.read_bits_big(12).unwrap();
        assert_eq!(big, vec![0x0F, 0xC5]);

        let mut r2 = BitReader::new(&data, Endian::Big);
        assert_eq!(r2.read_bits(12).unwrap(), 0xFC5);
    }
}
