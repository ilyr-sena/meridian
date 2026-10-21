//! Build-time font patching.
//!
//! iced 0.14 exposes no letter-spacing API (its `Text` widget has no tracking
//! control and the iced→cosmic-text conversion passes no such attribute), so
//! tightened typography is achieved at the font level instead: an embedded
//! face is patched at load time —
//!   * every `hmtx` advance width is scaled by [`TIGHTEN_FACTOR`] (-5%), and
//!   * the family is renamed so fontdb registers it as a distinct face next
//!     to the untouched original.
//!
//! Tight faces so far (see `theme::FONT_*_TIGHT` for the matching constants):
//!   * "Geist Mono Tight"   — chips only; body mono stays untouched.
//!
//! The patch is pure byte-level TTF surgery — no new dependencies:
//!   * `hmtx` widths are patched in place (same-width u16 swap);
//!   * the rebuilt `name` table is appended at EOF (growing it in place would
//!     overlap the next table) and the table-directory entry is repointed.
//!     Parsers (ttf-parser/fontdb) do not verify table checksums.
//! Verified end-to-end against FreeType/fontconfig: patched faces load with
//! their new families and measure exactly `TIGHTEN_FACTOR` of the original
//! advance.

use std::sync::LazyLock;

/// How much to tighten the advance widths (0.95 ⇒ -5% tracking).
const TIGHTEN_FACTOR: f32 = 0.95;

/// The specification of one tightened face.
struct TightFace {
    /// The raw embedded TTF to patch.
    bytes: &'static [u8],
    /// New family name, overwriting name-table IDs 1 (family) and 16
    /// (typographic family) — the only records fontdb indexes and matches on.
    family: &'static str,
    /// Cosmetic rename for PostScript-ish records (IDs 3 and 6).
    ps_replace: (&'static str, &'static str),
}

/// Geist Mono with tightened tracking for chips, patched from the embedded
/// face at build time. Falls back to the unpatched bytes (logged) on failure.
pub static GEIST_MONO_TIGHT: LazyLock<Vec<u8>> = LazyLock::new(|| {
    build_tight(TightFace {
        bytes: include_bytes!("../../assets/fonts/GeistMono-Regular.ttf"),
        family: "Geist Mono Tight",
        ps_replace: ("GeistMono", "GeistMonoTight"),
    })
});

fn build_tight(spec: TightFace) -> Vec<u8> {
    match patch_tight(&spec, TIGHTEN_FACTOR) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(
                "tight font patch failed for {}; using unpatched face: {e}",
                spec.family
            );
            spec.bytes.to_vec()
        }
    }
}

fn be16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn put16(d: &mut [u8], o: usize, v: u16) {
    d[o..o + 2].copy_from_slice(&v.to_be_bytes());
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn put32(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_be_bytes());
}

struct TableDir {
    offset: u32,
    /// Offset of the entry inside the table directory.
    dir_entry: usize,
}

fn find_table(data: &[u8], tag: &[u8; 4]) -> Option<TableDir> {
    let num = be16(data, 4)? as usize;
    (0..num)
        .map(|i| 12 + i * 16)
        .filter_map(|e| {
            (data.get(e..e + 4) == Some(&tag[..])).then(|| {
                Some(TableDir {
                    offset: be32(data, e + 8)?,
                    dir_entry: e,
                })
            })
        })
        .next()
        .flatten()
}

/// Rename one decoded name-table string for the given name ID.
fn rename_for(nid: u16, s: &str, spec: &TightFace) -> String {
    match nid {
        // Family + typographic family: the records fontdb matches on.
        1 | 16 => spec.family.to_string(),
        // Unique ID + PostScript name: cosmetic, keep them consistent.
        3 | 6 => s.replace(spec.ps_replace.0, spec.ps_replace.1),
        _ => s.to_string(),
    }
}

/// Tighten the advance widths and rename the family of a TTF.
fn patch_tight(spec: &TightFace, factor: f32) -> Result<Vec<u8>, String> {
    let raw = spec.bytes;
    let mut data = raw.to_vec();

    // --- 1. hmtx: tighten every nonzero advance width (in-place u16 swap) ---
    let hhea = find_table(&data, b"hhea").ok_or("missing hhea table")?;
    let hmtx = find_table(&data, b"hmtx").ok_or("missing hmtx table")?;
    let num_metrics =
        be16(&data, (hhea.offset + 34) as usize).ok_or("truncated hhea")? as usize;

    for i in 0..num_metrics {
        let o = hmtx.offset as usize + i * 4;
        let adv = be16(&data, o).ok_or("truncated hmtx")?;
        if adv > 0 {
            let tight = ((adv as f32) * factor).round().max(1.0) as u16;
            put16(&mut data, o, tight);
        }
    }

    // --- 2. name: rebuild with the renamed family ---
    let name = find_table(&data, b"name").ok_or("missing name table")?;
    let noff = name.offset as usize;
    let fmt = be16(&data, noff).ok_or("truncated name table")?;
    if fmt != 0 {
        return Err(format!("unsupported name-table format {fmt}"));
    }
    let count = be16(&data, noff + 2).ok_or("truncated name table")? as usize;
    let str_off = be16(&data, noff + 4).ok_or("truncated name table")? as usize;

    let mut heads: Vec<[u16; 4]> = Vec::with_capacity(count); // pid, eid, lid, nid
    let mut metas: Vec<[u16; 2]> = Vec::with_capacity(count); // pool offset, length
    let mut pool: Vec<u8> = Vec::with_capacity(1024);

    for i in 0..count {
        let r = noff + 6 + i * 12;
        let pid = be16(&data, r).ok_or("truncated name record")?;
        let eid = be16(&data, r + 2).ok_or("truncated name record")?;
        let lid = be16(&data, r + 4).ok_or("truncated name record")?;
        let nid = be16(&data, r + 6).ok_or("truncated name record")?;
        let len = be16(&data, r + 8).ok_or("truncated name record")? as usize;
        let off = be16(&data, r + 10).ok_or("truncated name record")? as usize;

        let s_off = noff + str_off + off;
        let bytes = data
            .get(s_off..s_off + len)
            .ok_or("name string out of bounds")?;

        let encoded: Vec<u8> = if pid == 0 || pid == 3 {
            // Unicode platforms store UTF-16BE.
            let s: String = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .filter_map(|u| char::from_u32(u as u32))
                .collect();
            rename_for(nid, &s, spec)
                .encode_utf16()
                .flat_map(|u| u.to_be_bytes())
                .collect()
        } else {
            // Legacy Mac strings: latin-1 round-trip is byte-exact (only the
            // family names are rewritten, and they are pure ASCII).
            let s: String = bytes.iter().map(|&b| b as char).collect();
            rename_for(nid, &s, spec)
                .chars()
                .map(|c| c as u8)
                .collect()
        };

        heads.push([pid, eid, lid, nid]);
        metas.push([pool.len() as u16, encoded.len() as u16]);
        pool.extend_from_slice(&encoded);
    }

    let string_offset = 6 + count * 12;
    let mut table = Vec::with_capacity(string_offset + pool.len());
    table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    table.extend_from_slice(&(count as u16).to_be_bytes());
    table.extend_from_slice(&(string_offset as u16).to_be_bytes());
    for ([pid, eid, lid, nid], [off, len]) in heads.into_iter().zip(metas) {
        for v in [pid, eid, lid, nid, len, off] {
            table.extend_from_slice(&v.to_be_bytes());
        }
    }
    table.extend_from_slice(&pool);

    // --- 3. append the rebuilt table at EOF and repoint the directory ---
    let new_off = data.len() as u32;
    data.extend_from_slice(&table);
    put32(&mut data, name.dir_entry + 8, new_off);
    put32(&mut data, name.dir_entry + 12, table.len() as u32);

    Ok(data)
}
