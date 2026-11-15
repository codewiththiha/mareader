//! Streaming ONNX protobuf reader: indexes weights, reads one at a time.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

use mareader_nn_core::wire;

use super::{EXTERNAL, FLOAT, FLOAT16, INT8, Node, UINT8, bad, node};
use crate::{Error, Result};

/// A chunk of `float_data` inside a `TensorProto`.
pub(super) enum FloatChunk {
    Span(u64, usize),
    Inline(u32),
}

/// One stored tensor's metadata and file spans, without holding its bulk bytes.
#[derive(Default)]
pub(super) struct TensorIndex {
    pub(super) dims: Vec<usize>,
    pub(super) dtype: u64,
    pub(super) raw: Option<(u64, usize)>,
    pub(super) floats: Vec<FloatChunk>,
    pub(super) ints: Vec<u64>,
    pub(super) external: bool,
}

/// Cursor over any `Read + Seek` ONNX stream, bounded by message end offsets.
pub(super) struct Stream<R> {
    inner: R,
    pos: u64,
    end: u64,
}

impl<R: Read + Seek> Stream<R> {
    pub(super) fn new(mut inner: R) -> Result<Self> {
        let end = inner.seek(SeekFrom::End(0)).map_err(io_err)?;
        inner.seek(SeekFrom::Start(0)).map_err(io_err)?;
        Ok(Self { inner, pos: 0, end })
    }

    fn seek_to(&mut self, at: u64) -> Result<()> {
        self.inner.seek(SeekFrom::Start(at)).map_err(io_err)?;
        self.pos = at;
        Ok(())
    }

    fn read_byte(&mut self, limit: u64) -> Result<u8> {
        if self.pos >= limit {
            return Err(bad("a number runs off the end"));
        }
        let mut byte = [0u8; 1];
        self.inner
            .read_exact(&mut byte)
            .map_err(|_| bad("a number runs off the end"))?;
        self.pos += 1;
        Ok(byte[0])
    }

    fn read_varint(&mut self, limit: u64) -> Result<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.read_byte(limit)?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(value);
            }
        }
        Err(bad("a number is too long"))
    }

    fn read_fixed32(&mut self, limit: u64) -> Result<u32> {
        let next = self
            .pos
            .checked_add(4)
            .filter(|&to| to <= limit)
            .ok_or_else(|| bad("a field runs off the end"))?;
        let mut buf = [0u8; 4];
        self.inner
            .read_exact(&mut buf)
            .map_err(|_| bad("a field runs off the end"))?;
        self.pos = next;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_len(&mut self, limit: u64) -> Result<usize> {
        let len = self.read_varint(limit)?;
        let len = usize::try_from(len).map_err(|_| bad("a length"))?;
        let next = self
            .pos
            .checked_add(len as u64)
            .filter(|&to| to <= limit)
            .ok_or_else(|| bad("a field runs off the end"))?;
        let _ = next;
        Ok(len)
    }

    fn skip(&mut self, len: u64, limit: u64) -> Result<()> {
        let next = self
            .pos
            .checked_add(len)
            .filter(|&to| to <= limit)
            .ok_or_else(|| bad("a field runs off the end"))?;
        self.seek_to(next)
    }

    fn read_exact_into(&mut self, len: usize, limit: u64, buf: &mut Vec<u8>) -> Result<()> {
        let next = self
            .pos
            .checked_add(len as u64)
            .filter(|&to| to <= limit)
            .ok_or_else(|| bad("a field runs off the end"))?;
        buf.resize(len, 0);
        if len > 0 {
            self.inner
                .read_exact(buf)
                .map_err(|_| bad("a field runs off the end"))?;
        }
        self.pos = next;
        Ok(())
    }

    fn read_span(&mut self, offset: u64, len: usize, buf: &mut Vec<u8>) -> Result<()> {
        buf.resize(len, 0);
        if len == 0 {
            return Ok(());
        }
        self.seek_to(offset)?;
        self.inner
            .read_exact(buf)
            .map_err(|_| bad("a field runs off the end"))?;
        self.pos += len as u64;
        Ok(())
    }

    fn append_span(&mut self, offset: u64, len: usize, buf: &mut Vec<u8>) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let old = buf.len();
        buf.resize(old + len, 0);
        self.seek_to(offset)?;
        if let Some(tail) = buf.get_mut(old..) {
            self.inner
                .read_exact(tail)
                .map_err(|_| bad("a field runs off the end"))?;
        }
        self.pos += len as u64;
        Ok(())
    }
}

fn io_err(e: std::io::Error) -> Error {
    Error::Load(format!("couldn't read the model file ({e})"))
}

fn skip_wire<R: Read + Seek>(stream: &mut Stream<R>, wire_kind: u64, limit: u64) -> Result<()> {
    match wire_kind {
        0 => {
            stream.read_varint(limit)?;
            Ok(())
        }
        1 => stream.skip(8, limit),
        2 => {
            let len = stream.read_varint(limit)?;
            stream.skip(len, limit)
        }
        5 => stream.skip(4, limit),
        _ => Err(bad("an unknown field kind")),
    }
}

/// Scans `ModelProto -> GraphProto` without loading tensor payloads into RAM.
pub(super) fn scan_graph<R: Read + Seek>(
    stream: &mut Stream<R>,
) -> Result<(Vec<Node>, HashMap<String, TensorIndex>)> {
    let file_end = stream.end;
    let mut graph = None;
    while stream.pos < file_end {
        let key = stream.read_varint(file_end)?;
        let (field, kind) = (key >> 3, key & 7);
        if field == 7 && kind == 2 {
            let len = stream.read_varint(file_end)?;
            let start = stream.pos;
            stream.skip(len, file_end)?;
            graph = Some((start, stream.pos));
        } else {
            skip_wire(stream, kind, file_end)?;
        }
    }
    let (graph_start, graph_end) =
        graph.ok_or_else(|| Error::Load("no model graph in the file".into()))?;
    stream.seek_to(graph_start)?;
    let mut nodes = Vec::new();
    let mut stores = HashMap::new();
    let mut scratch = Vec::new();
    while stream.pos < graph_end {
        let key = stream.read_varint(graph_end)?;
        let (field, kind) = (key >> 3, key & 7);
        match (field, kind) {
            (1, 2) => {
                let len = stream.read_len(graph_end)?;
                stream.read_exact_into(len, graph_end, &mut scratch)?;
                nodes.push(node(&scratch)?);
            }
            (5, 2) => {
                let len = stream.read_len(graph_end)?;
                let end = stream.pos + len as u64;
                let (name, idx) = scan_tensor(stream, end, &mut scratch)?;
                stores.insert(name, idx);
            }
            _ => skip_wire(stream, kind, graph_end)?,
        }
    }
    Ok((nodes, stores))
}

fn unpack_varints(packed: &[u8], out: &mut Vec<u64>) -> Result<()> {
    let mut pos = 0;
    while pos < packed.len() {
        out.push(wire::varint(packed, &mut pos).map_err(bad)?);
    }
    Ok(())
}

fn scan_tensor<R: Read + Seek>(
    stream: &mut Stream<R>,
    limit: u64,
    scratch: &mut Vec<u8>,
) -> Result<(String, TensorIndex)> {
    let mut name = String::new();
    let mut dims = Vec::new();
    let mut idx = TensorIndex::default();
    while stream.pos < limit {
        let key = stream.read_varint(limit)?;
        match (key >> 3, key & 7) {
            (1, 0) => dims.push(stream.read_varint(limit)?),
            (1, 2) => {
                let len = stream.read_len(limit)?;
                stream.read_exact_into(len, limit, scratch)?;
                unpack_varints(scratch, &mut dims)?;
            }
            (1, 5) | (5, 5) => return Err(bad("an integer list")),
            (2, 0) => idx.dtype = stream.read_varint(limit)?,
            (4, 2) => {
                let len = stream.read_len(limit)?;
                let start = stream.pos;
                stream.skip(len as u64, limit)?;
                idx.floats.push(FloatChunk::Span(start, len));
            }
            (4, 5) => idx
                .floats
                .push(FloatChunk::Inline(stream.read_fixed32(limit)?)),
            (5, 0) => idx.ints.push(stream.read_varint(limit)?),
            (5, 2) => {
                let len = stream.read_len(limit)?;
                stream.read_exact_into(len, limit, scratch)?;
                unpack_varints(scratch, &mut idx.ints)?;
            }
            (8, 2) => {
                let len = stream.read_len(limit)?;
                stream.read_exact_into(len, limit, scratch)?;
                name = std::str::from_utf8(scratch)
                    .map_err(|_| bad("a name"))?
                    .to_owned();
            }
            (9, 2) => {
                let len = stream.read_len(limit)?;
                let start = stream.pos;
                stream.skip(len as u64, limit)?;
                idx.raw = Some((start, len));
            }
            (14, 0) => idx.external = stream.read_varint(limit)? == EXTERNAL,
            (_, kind) => skip_wire(stream, kind, limit)?,
        }
    }
    idx.dims = dims
        .into_iter()
        .map(|d| usize::try_from(d).map_err(|_| bad("a dimension")))
        .collect::<Result<_>>()?;
    Ok((name, idx))
}

/// Streams one tensor from `stream` into `f32`, reusing `scratch`.
pub(super) fn decode_values<R: Read + Seek>(
    stream: &mut Stream<R>,
    name: &str,
    idx: &TensorIndex,
    scratch: &mut Vec<u8>,
) -> Result<Option<Vec<f32>>> {
    if !matches!(idx.dtype, FLOAT | FLOAT16 | UINT8 | INT8) {
        return Ok(None);
    }
    if idx.external {
        return Err(Error::Load(format!(
            "{name} is stored outside the model file"
        )));
    }
    let count = idx
        .dims
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| bad("a tensor size"))?;
    let raw_span = idx.raw.filter(|&(_, len)| len > 0);
    let low = |v: &u64| *v as u32;
    let out: Vec<f32> = if let Some((off, len)) = raw_span {
        stream.read_span(off, len, scratch)?;
        match idx.dtype {
            FLOAT => {
                let (words, _) = scratch.as_chunks::<4>();
                words.iter().map(|&w| f32::from_le_bytes(w)).collect()
            }
            FLOAT16 => {
                let (pairs, _) = scratch.as_chunks::<2>();
                pairs.iter().map(|&p| half(u16::from_le_bytes(p))).collect()
            }
            UINT8 => scratch.iter().map(|&b| f32::from(b)).collect(),
            _ => scratch.iter().map(|&b| f32::from(b as i8)).collect(),
        }
    } else {
        match idx.dtype {
            FLOAT => {
                scratch.clear();
                for chunk in &idx.floats {
                    match *chunk {
                        FloatChunk::Span(off, len) => stream.append_span(off, len, scratch)?,
                        FloatChunk::Inline(v) => scratch.extend_from_slice(&v.to_le_bytes()),
                    }
                }
                let (words, _) = scratch.as_chunks::<4>();
                words.iter().map(|&w| f32::from_le_bytes(w)).collect()
            }
            FLOAT16 => idx.ints.iter().map(|v| half(low(v) as u16)).collect(),
            UINT8 => idx.ints.iter().map(|v| f32::from(low(v) as u8)).collect(),
            _ => idx.ints.iter().map(|v| f32::from(low(v) as i8)).collect(),
        }
    };
    if out.len() != count {
        return Err(bad(&format!("{name} holds the wrong amount of data")));
    }
    Ok(Some(out))
}

/// A half-precision number, widened.
pub(super) fn half(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exp = u32::from((bits >> 10) & 0x1f);
    let frac = u32::from(bits & 0x3ff);
    match (exp, frac) {
        (0, 0) => f32::from_bits(sign),
        (0, f) => {
            let mag = f as f32 * (-24f32).exp2();
            if sign == 0 { mag } else { -mag }
        }
        (0x1f, 0) => f32::from_bits(sign | 0x7f80_0000),
        (0x1f, f) => f32::from_bits(sign | 0x7fc0_0000 | (f << 13)),
        (e, f) => f32::from_bits(sign | ((e + 112) << 23) | (f << 13)),
    }
}

/// `(q − zero) × scale` in place: whole, per first-axis slice, or per column.
pub(super) fn dequantise_in_place(
    name: &str,
    q: &mut [f32],
    scale: &[f32],
    zero: &[f32],
    columns: bool,
) -> Result<()> {
    let groups = scale.len();
    let zeros = zero.len();
    if groups == 0 || (zeros != groups && zeros != 1) || !q.len().is_multiple_of(groups) {
        return Err(bad(&format!("{name} has a scale that doesn't fit")));
    }
    let per = if columns {
        1
    } else {
        (q.len() / groups).max(1)
    };
    for (i, v) in q.iter_mut().enumerate() {
        let g = (i / per) % groups;
        let s = scale.get(g).copied().unwrap_or_default();
        let z = zero
            .get(if zeros == 1 { 0 } else { g })
            .copied()
            .unwrap_or_default();
        *v = (*v - z) * s;
    }
    Ok(())
}
