use crate::ExifError;

/// Scans a JPEG's marker segments and returns the bytes of the EXIF block
/// (everything after the "Exif\0\0" header inside the APP1 segment).
///
/// Stops scanning once it reaches the start-of-scan marker, since no more
/// metadata segments follow the image data.
pub fn find_exif_segment(data: &[u8]) -> Result<&[u8], ExifError> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(ExifError::NotJpeg);
    }

    let mut pos = 2;
    while pos + 1 < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = data[pos + 1];
        pos += 2;

        // Markers with no payload: standalone codes and restart markers.
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xDA {
            // Start of scan: image data follows, no more segments to check.
            break;
        }
        if pos + 2 > data.len() {
            break;
        }

        let seg_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > data.len() {
            return Err(ExifError::Malformed(
                "segment length runs past end of file".into(),
            ));
        }

        let payload = &data[pos + 2..pos + seg_len];
        if marker == 0xE1 && payload.len() >= 6 && &payload[0..6] == b"Exif\0\0" {
            return Ok(&payload[6..]);
        }

        pos += seg_len;
    }

    Err(ExifError::NoExif)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app1(payload: &[u8]) -> Vec<u8> {
        let mut seg = vec![0xFF, 0xE1];
        let len = (2 + payload.len()) as u16;
        seg.extend_from_slice(&len.to_be_bytes());
        seg.extend_from_slice(payload);
        seg
    }

    #[test]
    fn find_exif_segment_cases() {
        let valid_jpeg = {
            let mut d = vec![0xFF, 0xD8];
            d.extend_from_slice(&app1(b"Exif\0\0MM\x00*\x00\x00\x00\x08"));
            d.extend_from_slice(&[0xFF, 0xD9]);
            d
        };

        let jfif_no_exif = {
            let mut d = vec![0xFF, 0xD8];
            let payload = b"JFIF\0\x01\x01\x00\x00\x01\x00\x01\x00\x00";
            let mut app0 = vec![0xFF, 0xE0];
            let len = (2 + payload.len()) as u16;
            app0.extend_from_slice(&len.to_be_bytes());
            app0.extend_from_slice(payload);
            d.extend_from_slice(&app0);
            d.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x0C]);
            d
        };

        let overrun = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];

        let cases: Vec<(&str, Vec<u8>, fn(&Result<&[u8], ExifError>) -> bool)> = vec![
            (
                "missing SOI",
                vec![0x00, 0x01, 0x02, 0x03],
                |r| matches!(r, Err(ExifError::NotJpeg)),
            ),
            ("empty file", vec![], |r| {
                matches!(r, Err(ExifError::NotJpeg))
            }),
            ("valid exif segment", valid_jpeg, |r| {
                matches!(r, Ok(payload) if payload.starts_with(b"MM"))
            }),
            (
                "jfif app0 with no exif, hits start of scan",
                jfif_no_exif,
                |r| matches!(r, Err(ExifError::NoExif)),
            ),
            (
                "segment length overruns buffer",
                overrun,
                |r| matches!(r, Err(ExifError::Malformed(_))),
            ),
        ];

        for (name, data, check) in cases {
            let result = find_exif_segment(&data);
            assert!(
                check(&result),
                "case '{}' produced unexpected result: {:?}",
                name,
                result
            );
        }
    }
}
