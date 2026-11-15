//! Bounded primitives for reading protobuf-style voice model data.

/// Read an unsigned variable-length integer.
pub fn varint(buf: &[u8], pos: &mut usize) -> Result<u64, &'static str> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos).ok_or("a number runs off the end")?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Ok(value);
        }
    }
    Err("a number is too long")
}

/// Take `len` bytes and advance the cursor only when they all exist.
pub fn take<'a>(buf: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8], &'static str> {
    let end = pos.checked_add(len).ok_or("a length")?;
    let out = buf.get(*pos..end).ok_or("a field runs off the end")?;
    *pos = end;
    Ok(out)
}

/// A protobuf value needed by both voice model readers.
pub enum Value<'a> {
    Int(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

/// Append a repeated integer, written packed or one field at a time.
pub fn push_ints(value: &Value<'_>, out: &mut Vec<u64>) -> Result<(), &'static str> {
    match value {
        Value::Int(v) => out.push(*v),
        Value::Bytes(packed) => {
            let mut pos = 0;
            while pos < packed.len() {
                out.push(varint(packed, &mut pos)?);
            }
        }
        Value::Fixed32(_) => return Err("an integer list"),
    }
    Ok(())
}

/// Decode a bytes field as a name.
pub fn text(value: &Value<'_>) -> Result<String, &'static str> {
    match value {
        Value::Bytes(bytes) => String::from_utf8(bytes.to_vec()).map_err(|_| "a name"),
        _ => Err("a name"),
    }
}
