use crate::model::{Request, valid_request_id};
use crate::{FRAME_MAX, Result, WIRE_VERSION};
use serde_json::Value;
use std::io::{Read, Write};
use zeroize::Zeroizing;

pub(crate) fn read_raw(reader: &mut impl Read, max: usize) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let mut header = [0; 4];
    match reader.read(&mut header[..1])? {
        0 => return Ok(None),
        1 => (),
        _ => unreachable!(),
    }
    reader.read_exact(&mut header[1..])?;
    let size = u32::from_ne_bytes(header) as usize;
    if size == 0 || size > max || size > FRAME_MAX {
        return Err("invalid native frame length".into());
    }
    let mut bytes = Zeroizing::new(vec![0; size]);
    reader.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

pub(crate) fn read_frame(reader: &mut impl Read) -> Result<Option<Request>> {
    let Some(bytes) = read_raw(reader, FRAME_MAX)? else {
        return Ok(None);
    };
    decode_request(&bytes).map(Some)
}

pub(crate) fn decode_request(bytes: &[u8]) -> Result<Request> {
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "invalid native request")?;
    if request.v != WIRE_VERSION || !valid_request_id(&request.request) {
        return Err("invalid native request envelope".into());
    }
    Ok(request)
}

pub(crate) fn write_raw(writer: &mut impl Write, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > FRAME_MAX {
        return Err("outgoing native frame exceeds bound".into());
    }
    writer.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

pub(crate) fn write_frame(writer: &mut impl Write, value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    write_raw(writer, &bytes)
}
