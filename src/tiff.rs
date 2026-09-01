use crate::ExifError;

const TAG_EXIF_IFD_POINTER: u16 = 0x8769;
const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
const TAG_DATE_TIME: u16 = 0x0132;
const TYPE_ASCII: u16 = 2;

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn u16(self, b: &[u8]) -> u16 {
        match self {
            ByteOrder::Little => u16::from_le_bytes([b[0], b[1]]),
            ByteOrder::Big => u16::from_be_bytes([b[0], b[1]]),
        }
    }

    fn u32(self, b: &[u8]) -> u32 {
        match self {
            ByteOrder::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            ByteOrder::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
}

struct Ifd<'a> {
    data: &'a [u8],
    order: ByteOrder,
    entries: Vec<(u16, u16, u32, [u8; 4])>,
}

fn read_ifd(data: &[u8], order: ByteOrder, offset: usize) -> Result<Ifd<'_>, ExifError> {
    if offset + 2 > data.len() {
        return Err(ExifError::Malformed("ifd offset past end of data".into()));
    }
    let count = order.u16(&data[offset..offset + 2]) as usize;
    let entries_start = offset + 2;
    let entries_end = entries_start + count * 12;
    if entries_end > data.len() {
        return Err(ExifError::Malformed(
            "ifd entry table past end of data".into(),
        ));
    }

    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let e = &data[entries_start + i * 12..entries_start + i * 12 + 12];
        let tag = order.u16(&e[0..2]);
        let field_type = order.u16(&e[2..4]);
        let value_count = order.u32(&e[4..8]);
        let mut value = [0u8; 4];
        value.copy_from_slice(&e[8..12]);
        entries.push((tag, field_type, value_count, value));
    }

    Ok(Ifd {
        data,
        order,
        entries,
    })
}

fn ascii_value(ifd: &Ifd, tag: u16) -> Option<String> {
    let (_, field_type, count, value) = *ifd.entries.iter().find(|e| e.0 == tag)?;
    if field_type != TYPE_ASCII || count == 0 {
        return None;
    }
    let count = count as usize;
    let bytes: &[u8] = if count <= 4 {
        &value[..count]
    } else {
        let offset = ifd.order.u32(&value) as usize;
        if offset + count > ifd.data.len() {
            return None;
        }
        &ifd.data[offset..offset + count]
    };

    // EXIF ASCII strings are meant to be NUL-terminated, but not every
    // writer bothers, so treat a missing terminator as "use it all".
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let trimmed = &bytes[..end];
    if trimmed.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(trimmed).into_owned())
    }
}

/// Walks IFD0 and the Exif sub-IFD to find the raw capture-time string.
/// Returns it unparsed; `date::parse` is responsible for validating it.
pub fn find_capture_time_raw(data: &[u8]) -> Result<String, ExifError> {
    if data.len() < 8 {
        return Err(ExifError::Malformed("tiff header too short".into()));
    }
    let order = match &data[0..2] {
        b"II" => ByteOrder::Little,
        b"MM" => ByteOrder::Big,
        _ => {
            return Err(ExifError::Malformed(
                "unrecognized tiff byte order marker".into(),
            ))
        }
    };
    if order.u16(&data[2..4]) != 42 {
        return Err(ExifError::Malformed("bad tiff magic number".into()));
    }

    let ifd0_offset = order.u32(&data[4..8]) as usize;
    let ifd0 = read_ifd(data, order, ifd0_offset)?;

    let exif_ifd_offset = ifd0
        .entries
        .iter()
        .find(|e| e.0 == TAG_EXIF_IFD_POINTER)
        .map(|e| order.u32(&e.3) as usize);

    if let Some(sub_offset) = exif_ifd_offset {
        if let Ok(exif_ifd) = read_ifd(data, order, sub_offset) {
            if let Some(dt) = ascii_value(&exif_ifd, TAG_DATE_TIME_ORIGINAL) {
                return Ok(dt);
            }
        }
    }

    ascii_value(&ifd0, TAG_DATE_TIME).ok_or(ExifError::NoDateTag)
}
