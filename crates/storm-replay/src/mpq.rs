//! Bounded reading of an embedded MPQ file — port of `nom_mpq::MPQ::read_mpq_file_sector`
//! (nom-mpq 2.0.14, MIT) with a hard cap on the decompressed size.
//!
//! nom-mpq inflates every sector with `read_to_end` and no limit: a few MB of zlib/bzip2 in an
//! uploaded replay can expand to several GB and get the whole server OOM-killed (zip bomb).
//! Here every sector is inflated through a `Read::take` bounded by what is left of the
//! [`MAX_STREAM_BYTES`] budget, and the slicing of the sector table is bounds-checked (nom-mpq
//! indexes it directly and panics on a corrupted offset). The archive tables themselves are still
//! parsed by `nom_mpq::parser::parse`.

use crate::error::{Error, Result};
use std::io::Read;

/// Zip-bomb guard: maximum decompressed size of one embedded stream (256 MiB). The largest
/// stream of a real replay (`replay.game.events` of a long game) is a few MB, two orders of
/// magnitude below; anything beyond is rejected as an unreadable stream before it can exhaust
/// the memory of the process that decodes it.
pub const MAX_STREAM_BYTES: usize = 256 << 20;

const MPQ_FILE_COMPRESS: u32 = 0x0000_0200;
const MPQ_FILE_ENCRYPTED: u32 = 0x0001_0000;
const MPQ_FILE_SINGLE_UNIT: u32 = 0x0100_0000;
const MPQ_FILE_SECTOR_CRC: u32 = 0x0400_0000;
const MPQ_FILE_EXISTS: u32 = 0x8000_0000;

const COMPRESSION_PLAINTEXT: u8 = 0;
const COMPRESSION_ZLIB: u8 = 2;
const COMPRESSION_BZ2: u8 = 16;
const OTHER_COMPRESSION_PLAINTEXT: u8 = 47;

/// Reads the embedded file `name` (decompressed), at most [`MAX_STREAM_BYTES`].
pub(crate) fn read_file(mpq: &nom_mpq::MPQ, archive: &[u8], name: &'static str) -> Result<Vec<u8>> {
    read_file_capped(mpq, archive, name, MAX_STREAM_BYTES)
}

/// [`read_file`] with an explicit cap (tests).
pub(crate) fn read_file_capped(
    mpq: &nom_mpq::MPQ,
    archive: &[u8],
    name: &'static str,
    cap: usize,
) -> Result<Vec<u8>> {
    let err = |msg: String| Error::MissingStream(name, msg);
    let hash_entry = mpq
        .get_hash_table_entry(name)
        .map_err(|e| err(format!("{e:?}")))?;
    let block = mpq
        .block_table_entries
        .get(hash_entry.block_table_index as usize)
        .ok_or_else(|| err("block table index out of range".into()))?;
    if block.flags & MPQ_FILE_EXISTS == 0 || block.archived_size == 0 {
        return Ok(Vec::new());
    }
    if block.size as usize > cap {
        return Err(err(format!(
            "declared size {} B exceeds the {cap} B cap",
            block.size
        )));
    }
    if block.flags & MPQ_FILE_ENCRYPTED != 0 {
        return Err(err("encrypted file (unsupported)".into()));
    }
    let file_data = (block.offset as usize)
        .checked_add(mpq.archive_header.offset)
        .and_then(|start| Some(start..start.checked_add(block.archived_size as usize)?))
        .and_then(|range| archive.get(range))
        .ok_or_else(|| err("file data beyond the end of the archive".into()))?;
    let compressed = block.flags & MPQ_FILE_COMPRESS != 0;

    if block.flags & MPQ_FILE_SINGLE_UNIT != 0 {
        // compressed only when that saves at least a byte
        if compressed && block.size > block.archived_size {
            return decompress(file_data, cap).map_err(err);
        }
        return copy_capped(file_data, cap).map_err(err);
    }

    // Multi-sector file: table of `sectors + 1` u32 offsets (+1 with a CRC), then the sectors.
    let sector_size = 512u64
        .checked_shl(u32::from(mpq.archive_header.sector_size_shift))
        .filter(|&s| s > 0)
        .ok_or_else(|| err("invalid sector size shift".into()))?;
    let mut sectors = (u64::from(block.size) / sector_size) as usize + 1;
    let crc = block.flags & MPQ_FILE_SECTOR_CRC != 0;
    if crc {
        sectors += 1;
    }
    let table = file_data
        .get(..4 * (sectors + 1))
        .ok_or_else(|| err("sector table beyond the file data".into()))?;
    let positions: Vec<usize> = table
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c) as usize)
        .collect();
    let total_sectors = positions.len() - 1 - usize::from(crc);
    let mut out = Vec::new();
    let mut sector_bytes_left = block.size as usize;
    for window in positions[..=total_sectors].windows(2) {
        let sector = file_data
            .get(window[0]..window[1])
            .ok_or_else(|| err(format!("sector {}..{} out of range", window[0], window[1])))?;
        let budget = cap - out.len();
        let mut data = if compressed && sector_bytes_left > sector.len() {
            decompress(sector, budget).map_err(err)?
        } else {
            copy_capped(sector, budget).map_err(err)?
        };
        sector_bytes_left = sector_bytes_left.saturating_sub(data.len());
        out.append(&mut data);
    }
    Ok(out)
}

fn copy_capped(data: &[u8], cap: usize) -> std::result::Result<Vec<u8>, String> {
    if data.len() > cap {
        return Err(format!("data exceeds the {cap} B cap"));
    }
    Ok(data.to_vec())
}

/// Inflates one sector (first byte = compression type) into at most `cap` bytes.
fn decompress(input: &[u8], cap: usize) -> std::result::Result<Vec<u8>, String> {
    let (&kind, rest) = input
        .split_first()
        .ok_or_else(|| "empty compressed sector".to_owned())?;
    let mut out = Vec::new();
    // one byte past the cap to tell "exactly cap" from "more than cap"
    let limit = cap as u64 + 1;
    let read = match kind {
        COMPRESSION_PLAINTEXT | OTHER_COMPRESSION_PLAINTEXT => return copy_capped(rest, cap),
        COMPRESSION_ZLIB => flate2::read::ZlibDecoder::new(rest)
            .take(limit)
            .read_to_end(&mut out),
        COMPRESSION_BZ2 => bzip2_rs::DecoderReader::new(rest)
            .take(limit)
            .read_to_end(&mut out),
        other => return Err(format!("unsupported compression type {other}")),
    };
    read.map_err(|e| format!("decompression: {e}"))?;
    if out.len() > cap {
        return Err(format!(
            "decompressed size exceeds the {cap} B cap (zip bomb guard)"
        ));
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(vec![COMPRESSION_ZLIB], Default::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// The cap is enforced while inflating, whatever the size the archive declares.
    #[test]
    fn decompress_stops_at_the_cap() {
        let bomb = zlib(&vec![0u8; 1 << 20]);
        assert!(bomb.len() < 4096, "zeros compress well");
        let e = decompress(&bomb, 64 << 10).unwrap_err();
        assert!(e.contains("zip bomb"), "{e}");
        assert_eq!(
            decompress(&bomb, 1 << 20).unwrap().len(),
            1 << 20,
            "exactly the cap"
        );
        assert!(copy_capped(&[0; 10], 9).is_err());
        assert!(decompress(&[], 10).is_err());
        assert!(decompress(&[99, 1, 2], 10).is_err());
    }

    /// Same bytes as nom-mpq for every stream of every committed replay (port parity).
    #[test]
    fn parity_with_nom_mpq() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            let (_, mpq) = nom_mpq::parser::parse(&bytes).unwrap();
            for name in [
                "replay.details",
                "replay.initData",
                "replay.attributes.events",
                "replay.tracker.events",
                "replay.game.events",
                "replay.message.events",
                "replay.server.battlelobby",
            ] {
                let (_, theirs) = mpq.read_mpq_file_sector(name, false, &bytes).unwrap();
                let ours = read_file(&mpq, &bytes, name).unwrap();
                assert_eq!(ours, theirs, "{name}");
                if !ours.is_empty() {
                    // a cap one byte short of the stream rejects it
                    assert!(read_file_capped(&mpq, &bytes, name, ours.len() - 1).is_err());
                }
                checked += 1;
            }
        }
        assert!(checked >= 21, "{checked}");
    }
}
