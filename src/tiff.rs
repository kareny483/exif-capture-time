use crate::ExifError;

const TAG_EXIF_IFD_POINTER: u16 = 0x8769;
const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
const TAG_DATE_TIME: u16 = 0x0132;
const TAG_OFFSET_TIME: u16 = 0x9010;
const TAG_OFFSET_TIME_ORIGINAL: u16 = 0x9011;
const TAG_SUBSEC_TIME: u16 = 0x9290;
const TAG_SUBSEC_TIME_ORIGINAL: u16 = 0x9291;
const TYPE_ASCII: u16 = 2;

/// The raw strings backing a capture time, before `date::parse` validates
/// them. Subsecond and offset tags live in the Exif sub-IFD regardless of
/// whether the date itself came from DateTimeOriginal or IFD0's DateTime.
pub struct RawCaptureTime {
    pub date: String,
    pub subsec: Option<String>,
    pub offset: Option<String>,
}

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

/// Walks IFD0 and the Exif sub-IFD to find the raw capture-time strings.
/// Returns them unparsed; `date::parse` is responsible for validating them.
pub fn find_capture_time_raw(data: &[u8]) -> Result<RawCaptureTime, ExifError> {
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
    let exif_ifd = exif_ifd_offset.and_then(|off| read_ifd(data, order, off).ok());

    if let Some(exif_ifd) = &exif_ifd {
        if let Some(date) = ascii_value(exif_ifd, TAG_DATE_TIME_ORIGINAL) {
            return Ok(RawCaptureTime {
                date,
                subsec: ascii_value(exif_ifd, TAG_SUBSEC_TIME_ORIGINAL),
                offset: ascii_value(exif_ifd, TAG_OFFSET_TIME_ORIGINAL),
            });
        }
    }

    let date = ascii_value(&ifd0, TAG_DATE_TIME).ok_or(ExifError::NoDateTag)?;
    let (subsec, offset) = match &exif_ifd {
        Some(exif_ifd) => (
            ascii_value(exif_ifd, TAG_SUBSEC_TIME),
            ascii_value(exif_ifd, TAG_OFFSET_TIME),
        ),
        None => (None, None),
    };
    Ok(RawCaptureTime {
        date,
        subsec,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tag: u16, field_type: u16, count: u32, value: u32) -> [u8; 12] {
        let mut e = [0u8; 12];
        e[0..2].copy_from_slice(&tag.to_le_bytes());
        e[2..4].copy_from_slice(&field_type.to_le_bytes());
        e[4..8].copy_from_slice(&count.to_le_bytes());
        e[8..12].copy_from_slice(&value.to_le_bytes());
        e
    }

    fn ifd(entries: &[[u8; 12]]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for e in entries {
            buf.extend_from_slice(e);
        }
        buf
    }

    fn header(ifd0_offset: u32) -> Vec<u8> {
        let mut buf = vec![b'I', b'I'];
        buf.extend_from_slice(&42u16.to_le_bytes());
        buf.extend_from_slice(&ifd0_offset.to_le_bytes());
        buf
    }

    fn ascii_field(s: &str) -> Vec<u8> {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }

    #[test]
    fn header_too_short() {
        let data = vec![b'I', b'I', 42, 0, 0, 0];
        assert!(matches!(
            find_capture_time_raw(&data),
            Err(ExifError::Malformed(_))
        ));
    }

    #[test]
    fn unrecognized_byte_order_marker() {
        let mut data = vec![b'X', b'X'];
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        assert!(matches!(
            find_capture_time_raw(&data),
            Err(ExifError::Malformed(_))
        ));
    }

    #[test]
    fn bad_magic_number() {
        let mut data = vec![b'I', b'I'];
        data.extend_from_slice(&43u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        assert!(matches!(
            find_capture_time_raw(&data),
            Err(ExifError::Malformed(_))
        ));
    }

    #[test]
    fn ifd0_offset_past_end_of_data() {
        let data = header(1000);
        assert!(matches!(
            find_capture_time_raw(&data),
            Err(ExifError::Malformed(_))
        ));
    }

    #[test]
    fn ifd0_entry_table_truncated() {
        let mut data = header(8);
        // Claims two entries but only supplies bytes for one.
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&entry(TAG_DATE_TIME, TYPE_ASCII, 20, 0));
        assert!(matches!(
            find_capture_time_raw(&data),
            Err(ExifError::Malformed(_))
        ));
    }

    #[test]
    fn ifd0_date_time_success() {
        let mut data = header(8);
        let string_offset = 8 + 2 + 12; // header + count + one entry
        data.extend_from_slice(&ifd(&[entry(
            TAG_DATE_TIME,
            TYPE_ASCII,
            20,
            string_offset as u32,
        )]));
        data.extend_from_slice(&ascii_field("2023:07:04 14:22:09"));
        let got = find_capture_time_raw(&data).unwrap();
        assert_eq!(got.date, "2023:07:04 14:22:09");
        assert_eq!(got.subsec, None);
        assert_eq!(got.offset, None);
    }

    #[test]
    fn exif_sub_ifd_date_time_original_wins_over_ifd0_date_time() {
        let mut data = header(8);
        let decoy_offset = 8 + 2 + 24; // header + count + two entries
        let decoy = ascii_field("2000:01:01 00:00:00");
        let sub_ifd_offset = decoy_offset + decoy.len();
        let real_offset = sub_ifd_offset + 2 + 12; // sub ifd header + count + one entry

        data.extend_from_slice(&ifd(&[
            entry(TAG_EXIF_IFD_POINTER, 4, 1, sub_ifd_offset as u32),
            entry(TAG_DATE_TIME, TYPE_ASCII, 20, decoy_offset as u32),
        ]));
        data.extend_from_slice(&decoy);
        data.extend_from_slice(&ifd(&[entry(
            TAG_DATE_TIME_ORIGINAL,
            TYPE_ASCII,
            20,
            real_offset as u32,
        )]));
        data.extend_from_slice(&ascii_field("2023:07:04 14:22:09"));

        assert_eq!(find_capture_time_raw(&data).unwrap().date, "2023:07:04 14:22:09");
    }

    #[test]
    fn invalid_exif_ifd_pointer_falls_back_to_ifd0_date_time() {
        let mut data = header(8);
        let string_offset = 8 + 2 + 24; // header + count + two entries
        data.extend_from_slice(&ifd(&[
            entry(TAG_EXIF_IFD_POINTER, 4, 1, 999_999),
            entry(TAG_DATE_TIME, TYPE_ASCII, 20, string_offset as u32),
        ]));
        data.extend_from_slice(&ascii_field("2023:07:04 14:22:09"));

        assert_eq!(find_capture_time_raw(&data).unwrap().date, "2023:07:04 14:22:09");
    }

    #[test]
    fn subsec_and_offset_time_original_are_read_alongside_date_time_original() {
        let mut data = header(8);
        let exif_ifd_offset = 8 + 2 + 12; // header + count + one entry

        data.extend_from_slice(&ifd(&[entry(
            TAG_EXIF_IFD_POINTER,
            4,
            1,
            exif_ifd_offset as u32,
        )]));

        let entries_start = exif_ifd_offset + 2;
        let date_offset = entries_start + 3 * 12;
        let subsec_offset = date_offset + 20;
        let offset_string_offset = subsec_offset + 4;

        data.extend_from_slice(&ifd(&[
            entry(TAG_DATE_TIME_ORIGINAL, TYPE_ASCII, 20, date_offset as u32),
            entry(TAG_SUBSEC_TIME_ORIGINAL, TYPE_ASCII, 4, subsec_offset as u32),
            entry(
                TAG_OFFSET_TIME_ORIGINAL,
                TYPE_ASCII,
                7,
                offset_string_offset as u32,
            ),
        ]));
        data.extend_from_slice(&ascii_field("2023:07:04 14:22:09"));
        data.extend_from_slice(&ascii_field("500"));
        data.extend_from_slice(&ascii_field("-07:00"));

        let got = find_capture_time_raw(&data).unwrap();
        assert_eq!(got.date, "2023:07:04 14:22:09");
        assert_eq!(got.subsec.as_deref(), Some("500"));
        assert_eq!(got.offset.as_deref(), Some("-07:00"));
    }

    #[test]
    fn subsec_and_offset_time_fall_back_when_date_time_is_plain() {
        let mut data = header(8);
        let exif_ifd_offset = 8 + 2 + 24; // header + count + two entries
        let plain_date_offset = exif_ifd_offset + 2 + 24; // sub ifd header + count + two entries

        data.extend_from_slice(&ifd(&[
            entry(TAG_EXIF_IFD_POINTER, 4, 1, exif_ifd_offset as u32),
            entry(TAG_DATE_TIME, TYPE_ASCII, 20, plain_date_offset as u32),
        ]));

        let subsec_offset = plain_date_offset + 20;
        let offset_string_offset = subsec_offset + 4;

        data.extend_from_slice(&ifd(&[
            entry(TAG_SUBSEC_TIME, TYPE_ASCII, 4, subsec_offset as u32),
            entry(
                TAG_OFFSET_TIME,
                TYPE_ASCII,
                7,
                offset_string_offset as u32,
            ),
        ]));
        data.extend_from_slice(&ascii_field("2023:07:04 14:22:09"));
        data.extend_from_slice(&ascii_field("500"));
        data.extend_from_slice(&ascii_field("-07:00"));

        let got = find_capture_time_raw(&data).unwrap();
        assert_eq!(got.date, "2023:07:04 14:22:09");
        assert_eq!(got.subsec.as_deref(), Some("500"));
        assert_eq!(got.offset.as_deref(), Some("-07:00"));
    }

    #[test]
    fn ascii_value_offset_past_end_of_data_yields_no_date_tag() {
        let mut data = header(8);
        data.extend_from_slice(&ifd(&[entry(TAG_DATE_TIME, TYPE_ASCII, 20, 999_999)]));
        assert!(matches!(
            find_capture_time_raw(&data),
            Err(ExifError::NoDateTag)
        ));
    }

    #[test]
    fn big_endian_byte_order() {
        let mut data = vec![b'M', b'M'];
        data.extend_from_slice(&42u16.to_be_bytes());
        data.extend_from_slice(&8u32.to_be_bytes());

        let string_offset = 8 + 2 + 12u32;
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&TAG_DATE_TIME.to_be_bytes());
        data.extend_from_slice(&TYPE_ASCII.to_be_bytes());
        data.extend_from_slice(&20u32.to_be_bytes());
        data.extend_from_slice(&string_offset.to_be_bytes());
        data.extend_from_slice(&ascii_field("2023:07:04 14:22:09"));

        assert_eq!(find_capture_time_raw(&data).unwrap().date, "2023:07:04 14:22:09");
    }
}
