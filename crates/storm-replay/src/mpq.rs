//! Bounded reading of an embedded MPQ file — port of `nom_mpq::MPQ::read_mpq_file_sector`
//! (nom-mpq 2.0.14, MIT) with a hard cap on the decompressed size.
//!
//! The declared size is capped before allocating the output. Each sector is read directly
//! into its expected span of that buffer, with a one-byte probe for excess output. The sector
//! table is bounds-checked; the archive tables are still parsed by `nom_mpq::parser::parse`.

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
    if block.flags & MPQ_FILE_EXISTS == 0 {
        return Ok(Vec::new());
    }
    let size = block.size as usize;
    if size > cap {
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

    // Uncompressed files have no sector offset table. A compressed single unit is
    // stored raw whenever compressing would not save space.
    if !compressed || block.flags & MPQ_FILE_SINGLE_UNIT != 0 || file_data.is_empty() {
        let mut out = Vec::new();
        append_sector(&mut out, file_data, size, compressed).map_err(err)?;
        return Ok(out);
    }

    // Multi-sector file: `sectors + 1` u32 offsets (+1 with a CRC), then the sectors.
    let sector_size = sector_size(mpq.archive_header.sector_size_shift).map_err(err)?;
    let sectors = size.div_ceil(sector_size);
    let crc = block.flags & MPQ_FILE_SECTOR_CRC != 0;
    let table_len = 4 * (sectors + 1 + usize::from(crc));
    let table = file_data
        .get(..table_len)
        .ok_or_else(|| err("sector table beyond the file data".into()))?;
    let offsets = table.as_chunks::<4>().0;
    let offset = |index| u32::from_le_bytes(offsets[index]) as usize;
    let start = offset(0);
    let end = offset(sectors);
    if start < table_len
        || start > end
        || end > file_data.len()
        || offset(sectors + usize::from(crc)) != file_data.len()
    {
        return Err(err("invalid sector table bounds".into()));
    }
    let mut out = Vec::new();
    // Reserve once for this stream, not once per sector.
    out.try_reserve_exact(size)
        .map_err(|e| err(format!("output allocation: {e}")))?;
    for i in 0..sectors {
        let sector = file_data
            .get(offset(i)..offset(i + 1))
            .filter(|_| offset(i + 1) <= end)
            .ok_or_else(|| err(format!("sector {i} out of range")))?;
        let expected = sector_size.min(size - out.len());
        append_sector(&mut out, sector, expected, true).map_err(err)?;
    }
    Ok(out)
}

fn sector_size(shift: u16) -> std::result::Result<usize, String> {
    1usize
        .checked_shl(9 + u32::from(shift))
        .ok_or_else(|| "invalid sector size shift".into())
}

fn append_sector(
    out: &mut Vec<u8>,
    input: &[u8],
    expected: usize,
    compressed: bool,
) -> std::result::Result<(), String> {
    if input.len() == expected {
        out.try_reserve_exact(expected)
            .map_err(|e| format!("output allocation: {e}"))?;
        out.extend_from_slice(input);
        return Ok(());
    }
    if !compressed || input.is_empty() || input.len() > expected {
        return Err(format!(
            "invalid stored length: {} B for {expected} B sector",
            input.len()
        ));
    }
    out.try_reserve_exact(expected)
        .map_err(|e| format!("output allocation: {e}"))?;
    let start = out.len();
    out.resize(start + expected, 0);
    decompress(input, &mut out[start..])
}

/// Inflates one sector directly into its expected output span.
fn decompress(input: &[u8], out: &mut [u8]) -> std::result::Result<(), String> {
    let (&kind, rest) = input
        .split_first()
        .ok_or_else(|| "empty compressed sector".to_owned())?;
    match kind {
        COMPRESSION_PLAINTEXT | OTHER_COMPRESSION_PLAINTEXT => read_exact_sector(rest, out),
        COMPRESSION_ZLIB => read_exact_sector(flate2::read::ZlibDecoder::new(rest), out),
        COMPRESSION_BZ2 => read_exact_sector(bzip2_rs::DecoderReader::new(rest), out),
        other => Err(format!("unsupported compression type {other}")),
    }
}

fn read_exact_sector(mut reader: impl Read, out: &mut [u8]) -> std::result::Result<(), String> {
    reader
        .read_exact(out)
        .map_err(|e| format!("decompression: {e}"))?;
    // Read at most one more byte to detect overlong output or a decoder error at EOF.
    match reader
        .read(&mut [0])
        .map_err(|e| format!("decompression: {e}"))?
    {
        0 => Ok(()),
        _ => Err(format!(
            "decompressed sector exceeds its expected {} B",
            out.len()
        )),
    }
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

    #[test]
    fn decompress_requires_exact_output_length() {
        let plain = b"A small sector with ordinary text. A small sector with ordinary text.";
        let compressed = zlib(plain);
        let mut out = [0; 128];
        decompress(&compressed, &mut out[..plain.len()]).unwrap();
        assert_eq!(&out[..plain.len()], plain);
        assert!(decompress(&compressed, &mut out[..plain.len() - 1]).is_err());
        assert!(decompress(&compressed, &mut out[..plain.len() + 1]).is_err());
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
