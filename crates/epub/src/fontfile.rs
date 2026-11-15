//! Book fonts as plain OpenType: WOFF unpacked, obfuscated ones revealed.
use std::collections::HashMap;
use std::io::Read;

use quick_xml::Reader as Xml;
use quick_xml::events::Event;
use sha1::{Digest, Sha1};

use crate::href;

/// The largest font a file may unpack to.
const MAX_SFNT: usize = 64 << 20;
/// Bytes the IDPF and Adobe schemes hide at a font's start.
const IDPF_SPAN: usize = 1040;
const ADOBE_SPAN: usize = 1024;
const IDPF: &str = "http://www.idpf.org/2008/embedding";
const ADOBE: &str = "http://ns.adobe.com/pdf/enc#RC";

/// Tables WOFF2 names by their place in this list.
const KNOWN: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

/// How a book hid one of its fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hidden {
    Idpf,
    Adobe,
}

/// The font in `bytes` as OpenType, if it is one in any packing.
pub(crate) fn sfnt(bytes: Vec<u8>) -> Option<Vec<u8>> {
    let out = match bytes.get(..4)? {
        [0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf" => bytes,
        b"wOFF" => woff(&bytes)?,
        b"wOF2" => woff2(&bytes)?,
        _ => return None,
    };
    (out.len() <= MAX_SFNT).then_some(out)
}

/// Fonts `META-INF/encryption.xml` says are obfuscated, by archive path.
pub(crate) fn hidden(xml: &str) -> HashMap<String, Hidden> {
    let mut out = HashMap::new();
    let mut reader = Xml::from_str(xml);
    let (mut method, mut target) = (None, None);
    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        let element = match &event {
            Event::Start(e) | Event::Empty(e) => e,
            Event::End(e) if e.local_name().as_ref() == "EncryptedData" => {
                if let (Some(how), Some(path)) = (method.take(), target.take()) {
                    out.insert(path, how);
                }
                continue;
            }
            _ => continue,
        };
        let attr = |name: &str| {
            let found = element
                .attributes()
                .flatten()
                .find(|a| a.key.local_name().as_ref() == name);
            found.map(|a| a.value.into_owned())
        };
        match element.local_name().as_ref() {
            "EncryptedData" => (method, target) = (None, None),
            "EncryptionMethod" => {
                method = match attr("Algorithm").as_deref().map(str::trim) {
                    Some(IDPF) => Some(Hidden::Idpf),
                    Some(ADOBE) => Some(Hidden::Adobe),
                    _ => None,
                };
            }
            "CipherReference" => {
                target = attr("URI").and_then(|uri| href::resolve("/", &uri));
            }
            _ => {}
        }
    }
    out
}

/// The keys a book's identifiers give each scheme, likeliest first.
#[derive(Debug, Default)]
pub(crate) struct Keys {
    idpf: Vec<[u8; 20]>,
    adobe: Vec<[u8; 16]>,
}

impl Keys {
    /// From the package's unique identifier, then every other one.
    pub(crate) fn new<'a>(ids: impl Iterator<Item = &'a str>) -> Self {
        let mut keys = Self::default();
        for id in ids {
            let bare: String = id
                .chars()
                .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
                .collect();
            keys.idpf.push(Sha1::digest(bare.as_bytes()).into());
            if let Some(uuid) = uuid(&bare) {
                keys.adobe.push(uuid);
            }
        }
        keys
    }
}

/// The 16 bytes of a `urn:uuid:` identifier.
fn uuid(id: &str) -> Option<[u8; 16]> {
    let lower = id.to_ascii_lowercase();
    let rest = lower.strip_prefix("urn:uuid:").unwrap_or(&lower);
    let digits: Vec<u8> = rest.bytes().filter(|b| *b != b'-').collect();
    if digits.len() != 32 {
        return None;
    }
    let mut out = [0; 16];
    for (slot, pair) in out.iter_mut().zip(digits.chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// A hidden font revealed by the first key that yields a real font.
pub(crate) fn reveal(bytes: &[u8], how: Hidden, keys: &Keys) -> Option<Vec<u8>> {
    let head = <[u8; 4]>::try_from(bytes.get(..4)?).ok()?;
    let unmask = |key: &[u8], span: usize| {
        let mut probe = head;
        for (b, k) in probe.iter_mut().zip(key.iter().cycle()) {
            *b ^= k;
        }
        let mut out = match &probe {
            [0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf" | b"wOFF" | b"wOF2" => bytes.to_vec(),
            _ => return None,
        };
        for (b, k) in out.iter_mut().take(span).zip(key.iter().cycle()) {
            *b ^= k;
        }
        sfnt(out)
    };
    match how {
        Hidden::Idpf => keys.idpf.iter().find_map(|k| unmask(k, IDPF_SPAN)),
        Hidden::Adobe => keys.adobe.iter().find_map(|k| unmask(k, ADOBE_SPAN)),
    }
}

/// Big-endian reads that stop at the end of the data.
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.data.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(out)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        Some(u8::from_be_bytes(self.array()?))
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.array()?))
    }

    fn i16(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.array()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.array()?))
    }

    fn size(&mut self) -> Option<usize> {
        usize::try_from(self.u32()?).ok()
    }

    /// WOFF2's UIntBase128: up to five bytes, seven bits each.
    fn base128(&mut self) -> Option<usize> {
        let mut out: u32 = 0;
        for i in 0..5 {
            let byte = self.u8()?;
            if (i == 0 && byte == 0x80) || out & 0xFE00_0000 != 0 {
                return None;
            }
            out = (out << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return usize::try_from(out).ok();
            }
        }
        None
    }

    /// WOFF2's 255UInt16: one to three bytes.
    fn u255(&mut self) -> Option<usize> {
        let out = match self.u8()? {
            253 => self.u16()?,
            254 => u16::from(self.u8()?) + 506,
            255 => u16::from(self.u8()?) + 253,
            code => u16::from(code),
        };
        Some(usize::from(out))
    }
}

/// WOFF 1: each table zlib-packed, or stored as is.
fn woff(data: &[u8]) -> Option<Vec<u8>> {
    let mut r = Cursor::new(data);
    r.take(4)?;
    let flavor = r.u32()?;
    r.take(4)?;
    let count = r.u16()?;
    r.take(2)?;
    if r.size()? > MAX_SFNT {
        return None;
    }
    // Versions, then the metadata and private blocks' places.
    r.take(24)?;
    let mut tables = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let tag: [u8; 4] = r.array()?;
        let (offset, packed, length) = (r.size()?, r.size()?, r.size()?);
        r.take(4)?;
        let stored = data.get(offset..offset.checked_add(packed)?)?;
        let table = if packed == length {
            stored.to_vec()
        } else if packed < length && length <= MAX_SFNT {
            let mut out = Vec::with_capacity(length);
            let unzip = flate2::read::ZlibDecoder::new(stored);
            unzip.take(length as u64).read_to_end(&mut out).ok()?;
            (out.len() == length).then_some(out)?
        } else {
            return None;
        };
        tables.push((tag, table));
    }
    Some(assemble(flavor, tables))
}

/// One table in a WOFF2 directory.
struct Entry {
    tag: [u8; 4],
    /// Bytes in the unpacked stream: the transformed length if transformed.
    length: usize,
    transformed: bool,
}

/// WOFF2: one Brotli stream, `glyf`, `loca` and `hmtx` maybe transformed.
fn woff2(data: &[u8]) -> Option<Vec<u8>> {
    let mut r = Cursor::new(data);
    r.take(4)?;
    let flavor = r.u32()?;
    r.take(4)?;
    let count = r.u16()?;
    r.take(2)?;
    if r.size()? > MAX_SFNT || &flavor.to_be_bytes() == b"ttcf" {
        return None;
    }
    let packed = r.size()?;
    // Versions, then the metadata and private blocks' places.
    r.take(24)?;
    let mut entries = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let flags = r.u8()?;
        let tag = match flags & 0x3F {
            0x3F => r.array()?,
            known => **KNOWN.get(usize::from(known))?,
        };
        let version = flags >> 6;
        let length = r.base128()?;
        let transformed = match &tag {
            b"glyf" | b"loca" => version != 3,
            _ => version != 0,
        };
        let length = if transformed { r.base128()? } else { length };
        entries.push(Entry {
            tag,
            length,
            transformed,
        });
    }
    let want = entries
        .iter()
        .try_fold(0usize, |sum, e| sum.checked_add(e.length))?;
    if want > MAX_SFNT {
        return None;
    }
    let mut stream = Vec::with_capacity(want);
    let unpack = brotli_decompressor::Decompressor::new(r.take(packed)?, 4096);
    unpack.take(want as u64).read_to_end(&mut stream).ok()?;
    if stream.len() < want {
        return None;
    }
    let mut parts = Vec::with_capacity(entries.len());
    let mut at = 0;
    for e in &entries {
        parts.push(stream.get(at..at + e.length)?);
        at += e.length;
    }
    let raw = |tag: &[u8; 4]| {
        let i = entries
            .iter()
            .position(|e| &e.tag == tag && !e.transformed)?;
        parts.get(i).copied()
    };
    let glyf_at = entries
        .iter()
        .position(|e| &e.tag == b"glyf" && e.transformed);
    let glyphs = match glyf_at {
        Some(i) => Some(rebuild_glyf(parts.get(i)?)?),
        None => None,
    };
    let mut tables = Vec::with_capacity(entries.len());
    for (e, part) in entries.iter().zip(&parts) {
        let table = match (&e.tag, e.transformed, &glyphs) {
            (_, false, _) => part.to_vec(),
            (b"glyf", true, Some(g)) => g.glyf.clone(),
            (b"loca", true, Some(g)) => g.loca.clone(),
            (b"hmtx", true, Some(g)) => {
                let metrics = raw(b"hhea")?.get(34..36)?;
                let metrics = usize::from(u16::from_be_bytes(metrics.try_into().ok()?));
                rebuild_hmtx(part, metrics, &g.x_mins)?
            }
            _ => return None,
        };
        tables.push((e.tag, table));
    }
    Some(assemble(flavor, tables))
}

/// A rebuilt `glyf`, its `loca`, and each glyph's left edge.
struct Glyphs {
    glyf: Vec<u8>,
    loca: Vec<u8>,
    x_mins: Vec<i16>,
}

/// The transformed `glyf` table's seven streams, read back into glyphs.
fn rebuild_glyf(data: &[u8]) -> Option<Glyphs> {
    let mut r = Cursor::new(data);
    r.take(2)?;
    let options = r.u16()?;
    let count = usize::from(r.u16()?);
    let long = r.u16()? != 0;
    let mut sizes = [0usize; 7];
    for size in &mut sizes {
        *size = r.size()?;
    }
    let [contours, points, flags, glyph, composite, bbox, code] = sizes;
    let mut contours = Cursor::new(r.take(contours)?);
    let mut points = Cursor::new(r.take(points)?);
    let mut flags = Cursor::new(r.take(flags)?);
    let mut glyph = Cursor::new(r.take(glyph)?);
    let mut composite = Cursor::new(r.take(composite)?);
    let mut bbox = Cursor::new(r.take(bbox)?);
    let mut code = Cursor::new(r.take(code)?);
    let overlaps = match options & 1 {
        0 => None,
        _ => Some(r.take(count.div_ceil(8))?),
    };
    let boxed = bbox.take(count.div_ceil(32) * 4)?;
    let has = |bits: &[u8], g: usize| bits.get(g / 8).is_some_and(|b| b & (0x80 >> (g % 8)) != 0);
    let mut out = Glyphs {
        glyf: Vec::new(),
        loca: Vec::new(),
        x_mins: Vec::with_capacity(count),
    };
    let mut offsets = Vec::with_capacity(count + 1);
    for g in 0..count {
        offsets.push(out.glyf.len());
        let n = contours.i16()?;
        let given = if has(boxed, g) {
            Some(bbox.array::<8>()?)
        } else {
            None
        };
        let x_min = match n {
            0 => 0,
            ..0 => {
                let given = given?;
                let start = composite.at;
                let mut program = false;
                loop {
                    let flags = composite.u16()?;
                    composite.take(2)?;
                    let args = if flags & 0x0001 != 0 { 4 } else { 2 };
                    let scale = match flags {
                        f if f & 0x0008 != 0 => 2,
                        f if f & 0x0040 != 0 => 4,
                        f if f & 0x0080 != 0 => 8,
                        _ => 0,
                    };
                    composite.take(args + scale)?;
                    program |= flags & 0x0100 != 0;
                    if flags & 0x0020 == 0 {
                        break;
                    }
                }
                out.glyf.extend((-1i16).to_be_bytes());
                out.glyf.extend(given);
                out.glyf.extend(composite.data.get(start..composite.at)?);
                if program {
                    let length = glyph.u255()?;
                    out.glyf.extend(u16::try_from(length).ok()?.to_be_bytes());
                    out.glyf.extend(code.take(length)?);
                }
                let [a, b, ..] = given;
                i16::from_be_bytes([a, b])
            }
            _ => {
                let overlap = overlaps.is_some_and(|bits| has(bits, g));
                let parts = (&mut points, &mut flags, &mut glyph, &mut code);
                simple(&mut out.glyf, n, given, overlap, parts)?
            }
        };
        out.x_mins.push(x_min);
        out.glyf.resize(out.glyf.len().next_multiple_of(4), 0);
        if out.glyf.len() > MAX_SFNT {
            return None;
        }
    }
    offsets.push(out.glyf.len());
    for offset in offsets {
        if long {
            out.loca.extend(u32::try_from(offset).ok()?.to_be_bytes());
        } else {
            out.loca
                .extend(u16::try_from(offset / 2).ok()?.to_be_bytes());
        }
    }
    Some(out)
}

/// The streams a simple glyph reads: point counts, flags, moves, programs.
type Streams<'s, 'd> = (
    &'s mut Cursor<'d>,
    &'s mut Cursor<'d>,
    &'s mut Cursor<'d>,
    &'s mut Cursor<'d>,
);

/// Writes a simple glyph of `n` contours; returns its left edge.
fn simple(
    out: &mut Vec<u8>,
    n: i16,
    given: Option<[u8; 8]>,
    overlap: bool,
    (points, flags, glyph, code): Streams<'_, '_>,
) -> Option<i16> {
    let mut ends = Vec::with_capacity(usize::try_from(n).ok()?);
    let mut total = 0usize;
    for _ in 0..n {
        total += points.u255()?;
        ends.push(u16::try_from(total.checked_sub(1)?).ok()?);
    }
    let mut at = Vec::with_capacity(total);
    let (mut x, mut y) = (0i32, 0i32);
    for &flag in flags.take(total)? {
        let (dx, dy) = triplet(flag & 0x7F, glyph)?;
        (x, y) = (x + dx, y + dy);
        at.push((
            i16::try_from(x).ok()?,
            i16::try_from(y).ok()?,
            flag & 0x80 == 0,
        ));
    }
    let length = glyph.u255()?;
    let program = code.take(length)?;
    let bounds = match given {
        Some(b) => b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| i16::from_be_bytes(*p))
            .collect(),
        None => {
            let (xs, ys) = (at.iter().map(|p| p.0), at.iter().map(|p| p.1));
            let (x0, x1) = (xs.clone().min().unwrap_or(0), xs.max().unwrap_or(0));
            let (y0, y1) = (ys.clone().min().unwrap_or(0), ys.max().unwrap_or(0));
            vec![x0, y0, x1, y1]
        }
    };
    out.extend(n.to_be_bytes());
    for b in &bounds {
        out.extend(b.to_be_bytes());
    }
    for end in ends {
        out.extend(end.to_be_bytes());
    }
    out.extend(u16::try_from(length).ok()?.to_be_bytes());
    out.extend(program);
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let mut last = (0i16, 0i16);
    for (i, &(px, py, on)) in at.iter().enumerate() {
        let mut flag = u8::from(on);
        if i == 0 && overlap {
            flag |= 0x40;
        }
        flag |= coordinate(i32::from(px) - i32::from(last.0), 0x02, 0x10, &mut xs);
        flag |= coordinate(i32::from(py) - i32::from(last.1), 0x04, 0x20, &mut ys);
        out.push(flag);
        last = (px, py);
    }
    out.extend(xs);
    out.extend(ys);
    Some(bounds.first().copied().unwrap_or(0))
}

/// One coordinate delta in `glyf`'s packing; the flag bits it needs.
fn coordinate(delta: i32, short: u8, same: u8, out: &mut Vec<u8>) -> u8 {
    if delta == 0 {
        return same;
    }
    if let Ok(small) = u8::try_from(delta.unsigned_abs()) {
        out.push(small);
        return short | if delta > 0 { same } else { 0 };
    }
    let wide = i16::try_from(delta).unwrap_or(0);
    out.extend(wide.to_be_bytes());
    0
}

/// A point's move, packed by WOFF2 in one to four bytes after its flag.
fn triplet(flag: u8, glyph: &mut Cursor<'_>) -> Option<(i32, i32)> {
    let sign = |bit: u8, v: i32| if bit & 1 != 0 { v } else { -v };
    let f = i32::from(flag);
    let byte = |c: &mut Cursor<'_>| c.u8().map(i32::from);
    Some(match flag {
        0..10 => (0, sign(flag, ((f & 14) << 7) + byte(glyph)?)),
        10..20 => (sign(flag, (((f - 10) & 14) << 7) + byte(glyph)?), 0),
        20..84 => {
            let (b0, b1) = (f - 20, byte(glyph)?);
            let dx = 1 + (b0 & 0x30) + (b1 >> 4);
            let dy = 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F);
            (sign(flag, dx), sign(flag >> 1, dy))
        }
        84..120 => {
            let b0 = f - 84;
            let dx = 1 + ((b0 / 12) << 8) + byte(glyph)?;
            let dy = 1 + (((b0 % 12) >> 2) << 8) + byte(glyph)?;
            (sign(flag, dx), sign(flag >> 1, dy))
        }
        120..124 => {
            let (b0, b1, b2) = (byte(glyph)?, byte(glyph)?, byte(glyph)?);
            let dx = (b0 << 4) + (b1 >> 4);
            let dy = ((b1 & 0x0F) << 8) + b2;
            (sign(flag, dx), sign(flag >> 1, dy))
        }
        _ => {
            let dx = i32::from(glyph.u16()?);
            let dy = i32::from(glyph.u16()?);
            (sign(flag, dx), sign(flag >> 1, dy))
        }
    })
}

/// The transformed `hmtx`: side bearings left out are the glyphs' left edges.
fn rebuild_hmtx(data: &[u8], metrics: usize, x_mins: &[i16]) -> Option<Vec<u8>> {
    let mut r = Cursor::new(data);
    let flags = r.u8()?;
    if flags & 0xFC != 0 || flags & 3 == 0 {
        return None;
    }
    let count = x_mins.len();
    let metrics = metrics.min(count);
    let mut advances = Vec::with_capacity(metrics);
    for _ in 0..metrics {
        advances.push(r.u16()?);
    }
    let mut bearings = Vec::with_capacity(count);
    for (g, &x_min) in x_mins.iter().enumerate() {
        let stored = if g < metrics {
            flags & 1 == 0
        } else {
            flags & 2 == 0
        };
        bearings.push(if stored { r.i16()? } else { x_min });
    }
    let mut out = Vec::with_capacity(metrics * 2 + count * 2);
    for (g, bearing) in bearings.iter().enumerate() {
        if let Some(advance) = advances.get(g) {
            out.extend(advance.to_be_bytes());
        }
        out.extend(bearing.to_be_bytes());
    }
    Some(out)
}

/// An OpenType file of `tables`, sorted by tag, each padded to four bytes.
fn assemble(flavor: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|t| t.0);
    let count = u16::try_from(tables.len()).unwrap_or(u16::MAX);
    let log = count.max(1).ilog2();
    let range = (1u16 << log).saturating_mul(16);
    let mut out = Vec::new();
    out.extend(flavor.to_be_bytes());
    out.extend(count.to_be_bytes());
    out.extend(range.to_be_bytes());
    out.extend(u16::try_from(log).unwrap_or(0).to_be_bytes());
    out.extend(count.saturating_mul(16).saturating_sub(range).to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        out.extend(tag);
        out.extend(checksum(data).to_be_bytes());
        out.extend(u32::try_from(offset).unwrap_or(0).to_be_bytes());
        out.extend(u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

/// The OpenType table checksum: big-endian words summed, wrapping.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, word| {
        let mut padded = [0; 4];
        for (p, b) in padded.iter_mut().zip(word) {
            *p = *b;
        }
        sum.wrapping_add(u32::from_be_bytes(padded))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_numbers_read_as_woff2_writes_them() {
        assert_eq!(Cursor::new(&[0x3F]).base128(), Some(63));
        let max = [0x8F, 0xFF, 0xFF, 0xFF, 0x7F];
        assert_eq!(Cursor::new(&max).base128(), Some(u32::MAX as usize));
        assert_eq!(Cursor::new(&[0x80, 0x3F]).base128(), None);
        assert_eq!(Cursor::new(&[0x90, 0x80, 0x80, 0x80, 0x00]).base128(), None);
        assert_eq!(Cursor::new(&[0xFF; 6]).base128(), None);
        for bytes in [&[254, 0][..], &[255, 253], &[253, 1, 250]] {
            assert_eq!(Cursor::new(bytes).u255(), Some(506));
        }
        assert_eq!(Cursor::new(&[252]).u255(), Some(252));
        assert_eq!(Cursor::new(&[253, 1]).u255(), None);
    }

    #[test]
    fn point_moves_unpack_in_every_width() {
        let mv = |flag: u8, bytes: &[u8]| triplet(flag, &mut Cursor::new(bytes));
        assert_eq!(mv(1, &[5]), Some((0, 5)));
        assert_eq!(mv(0, &[5]), Some((0, -5)));
        assert_eq!(mv(11, &[7]), Some((7, 0)));
        // One byte, two nibbles, each one more than stored.
        assert_eq!(mv(23, &[0x12]), Some((2, 3)));
        assert_eq!(mv(84, &[1, 2]), Some((-2, -3)));
        assert_eq!(mv(123, &[0x12, 0x34, 0x56]), Some((0x123, 0x456)));
        assert_eq!(mv(127, &[1, 0, 2, 0]), Some((256, 512)));
        assert_eq!(mv(127, &[1, 0]), None);
    }

    #[test]
    fn obfuscated_font_lists_are_read_from_the_container() {
        let xml = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container"
            xmlns:enc="http://www.w3.org/2001/04/xmlenc#">
          <enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>
            <enc:CipherData><enc:CipherReference URI="OEBPS/fonts/A%20B.otf"/></enc:CipherData>
          </enc:EncryptedData>
          <enc:EncryptedData><enc:EncryptionMethod Algorithm="http://ns.adobe.com/pdf/enc#RC"/>
            <enc:CipherData><enc:CipherReference URI="OEBPS/fonts/c.ttf"/></enc:CipherData>
          </enc:EncryptedData>
          <enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>
            <enc:CipherData><enc:CipherReference URI="OEBPS/locked.xhtml"/></enc:CipherData>
          </enc:EncryptedData>
        </encryption>"#;
        let found = hidden(xml);
        assert_eq!(found.len(), 2);
        assert_eq!(found.get("/OEBPS/fonts/A B.otf"), Some(&Hidden::Idpf));
        assert_eq!(found.get("/OEBPS/fonts/c.ttf"), Some(&Hidden::Adobe));
        assert!(hidden("<broken").is_empty());
    }

    #[test]
    fn hidden_fonts_come_back_with_the_right_key_only() {
        let mut font = vec![0u8, 1, 0, 0];
        font.extend((0..2000u32).map(|i| (i % 251) as u8));
        let id = " urn:uuid:12345678-9abc-def0-1234-56789abcdef0\n";
        let keys = Keys::new(["other", id].into_iter());
        assert_eq!(keys.adobe.len(), 1);
        let hide = |key: &[u8], span: usize| {
            let mut out = font.clone();
            for (b, k) in out.iter_mut().take(span).zip(key.iter().cycle()) {
                *b ^= k;
            }
            out
        };
        let idpf: [u8; 20] = Sha1::digest(id.trim().as_bytes()).into();
        let hidden = hide(&idpf, IDPF_SPAN);
        assert_eq!(reveal(&hidden, Hidden::Idpf, &keys).as_ref(), Some(&font));
        let uuid = uuid(id.trim()).unwrap_or_default();
        assert_eq!(uuid.first(), Some(&0x12));
        let hidden = hide(&uuid, ADOBE_SPAN);
        assert_eq!(reveal(&hidden, Hidden::Adobe, &keys).as_ref(), Some(&font));
        // The wrong scheme's keys find nothing.
        assert_eq!(reveal(&hidden, Hidden::Idpf, &keys), None);
    }

    #[test]
    fn plain_fonts_pass_and_others_do_not() {
        assert_eq!(sfnt(b"OTTO1234".to_vec()), Some(b"OTTO1234".to_vec()));
        assert_eq!(sfnt(b"wOFFjunk".to_vec()), None);
        assert_eq!(sfnt(b"wOF2junk".to_vec()), None);
        assert_eq!(sfnt(b"GIF89a".to_vec()), None);
        assert_eq!(sfnt(Vec::new()), None);
    }

    /// A file from the fonts test book.
    fn shipped(name: &str) -> Option<Vec<u8>> {
        let book = include_bytes!("../tests/books/fonts.epub");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&book[..])).ok()?;
        let mut file = zip.by_name(&format!("OEBPS/fonts/{name}")).ok()?;
        let mut out = Vec::new();
        file.read_to_end(&mut out).ok()?;
        Some(out)
    }

    /// Each glyph's outline, advance and side bearing, as skrifa reads them.
    fn glyphs(font: &[u8]) -> Vec<(String, Option<f32>, Option<f32>)> {
        use skrifa::instance::{LocationRef, Size};
        use skrifa::outline::{DrawSettings, pen::SvgPen};
        use skrifa::raw::TableProvider;
        use skrifa::{FontRef, GlyphId, MetadataProvider};
        let Ok(font) = FontRef::new(font) else {
            return Vec::new();
        };
        let count = font.maxp().map_or(0, |m| m.num_glyphs());
        let outlines = font.outline_glyphs();
        let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
        (0..count)
            .map(|g| {
                let (id, mut pen) = (GlyphId::new(u32::from(g)), SvgPen::new());
                let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
                let drawn = outlines.get(id).map(|o| o.draw(settings, &mut pen).is_ok());
                let (adv, lsb) = (metrics.advance_width(id), metrics.left_side_bearing(id));
                (format!("{drawn:?} {pen}"), adv, lsb)
            })
            .collect()
    }

    #[test]
    fn packed_and_hidden_fonts_unpack_to_the_same_glyphs() {
        let ids = [
            "urn:isbn:9780000000002",
            "urn:uuid:5c0ffee0-1234-4abc-9def-0123456789ab",
        ];
        let keys = Keys::new(ids.into_iter());
        let open = |name: &str, how: Option<Hidden>| {
            let bytes = shipped(name)?;
            match how {
                Some(how) => reveal(&bytes, how, &keys),
                None => sfnt(bytes),
            }
        };
        let sans = open("hidden-sans.ttf", Some(Hidden::Idpf)).unwrap_or_default();
        let mono = open("hidden-mono.ttf", Some(Hidden::Adobe)).unwrap_or_default();
        let (sans, mono) = (glyphs(&sans), glyphs(&mono));
        // Composites, programs and overlap flags are among them.
        assert_eq!((sans.len(), mono.len()), (191, 245));
        assert!(
            sans.iter()
                .all(|g| g.1.is_some() && !g.0.starts_with("None"))
        );
        // Each font's glyph count and first differing glyph, all reported.
        let mut found = Vec::new();
        for (name, original) in [
            ("packed-sans.woff", &sans),
            ("packed-sans.woff2", &sans),
            ("packed-mono.woff2", &mono),
        ] {
            let unpacked = glyphs(&open(name, None).unwrap_or_default());
            let differs = unpacked
                .iter()
                .zip(original.iter())
                .position(|(a, b)| a != b);
            let first = differs.and_then(|g| Some((g, unpacked.get(g)?, original.get(g)?)));
            found.push((name, unpacked.len(), format!("{first:?}")));
        }
        let none = "None".to_owned();
        let expected = [
            ("packed-sans.woff", 191, none.clone()),
            ("packed-sans.woff2", 191, none.clone()),
            ("packed-mono.woff2", 245, none),
        ];
        assert_eq!(found, expected);
        // Hidden fonts read without their key are not fonts.
        assert_eq!(shipped("hidden-sans.ttf").and_then(sfnt), None);
    }

    #[test]
    fn tables_assemble_sorted_with_checksums() {
        let font = assemble(
            0x0001_0000,
            vec![(*b"name", vec![1, 2, 3]), (*b"cmap", vec![0; 8])],
        );
        assert_eq!(font.get(4..6), Some(&[0, 2][..]));
        assert_eq!(font.get(12..16), Some(&b"cmap"[..]));
        assert_eq!(font.get(28..32), Some(&b"name"[..]));
        assert_eq!(checksum(&[1, 2, 3]), 0x0102_0300);
        assert_eq!(font.len(), 12 + 32 + 8 + 4);
    }
}
