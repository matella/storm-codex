//! Décodeur de `replay.attributes.events` — port de `decode_replay_attributes_events`
//! (protocolXXXXX.py). Seul stream en little-endian.

use crate::bitpacked::{BitReader, Endian};
use crate::error::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct AttributeValue {
    pub namespace: u32,
    pub attrid: u32,
    /// 4 octets lus, inversés, dépouillés des `\x00` des deux côtés (référence Python
    /// `[::-1].strip(b'\x00')`) — typiquement un code ASCII court (ex. `b"5v5"`).
    pub value: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Attributes {
    pub source: u8,
    pub map_namespace: u32,
    /// scope (16 = global, 1–15 = slot joueur) → attrid → valeurs.
    pub scopes: HashMap<u8, HashMap<u32, Vec<AttributeValue>>>,
}

pub fn decode_attributes(content: &[u8]) -> Result<Attributes> {
    let mut buf = BitReader::new(content, Endian::Little);
    let mut attrs = Attributes::default();
    if buf.done() {
        return Ok(attrs);
    }
    attrs.source = buf.read_bits(8)? as u8;
    attrs.map_namespace = buf.read_bits(32)? as u32;
    let _count = buf.read_bits(32)?;
    while !buf.done() {
        let namespace = buf.read_bits(32)? as u32;
        let attrid = buf.read_bits(32)? as u32;
        let scope = buf.read_bits(8)? as u8;
        let raw = buf.read_aligned_bytes(4)?;
        let mut value: Vec<u8> = raw.iter().rev().copied().collect();
        let start = value.iter().position(|&b| b != 0).unwrap_or(value.len());
        let end = value.iter().rposition(|&b| b != 0).map_or(start, |p| p + 1);
        value = value[start..end].to_vec();
        attrs
            .scopes
            .entry(scope)
            .or_default()
            .entry(attrid)
            .or_default()
            .push(AttributeValue { namespace, attrid, value });
    }
    Ok(attrs)
}
