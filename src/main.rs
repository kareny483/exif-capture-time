use std::env;
use std::fmt;
use std::fs;
use std::process;

mod date;
mod jpeg;
mod tiff;

#[derive(Debug)]
enum ExifError {
    Io(std::io::Error),
    UnsupportedFormat,
    NoExif,
    NoDateTag,
    Malformed(String),
}

impl fmt::Display for ExifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExifError::Io(e) => write!(f, "could not read file: {}", e),
            ExifError::UnsupportedFormat => write!(
                f,
                "not a JPEG or TIFF-based file (no SOI marker or TIFF byte order marker found)"
            ),
            ExifError::NoExif => write!(f, "no EXIF (APP1) segment found"),
            ExifError::NoDateTag => write!(f, "EXIF data has no DateTimeOriginal or DateTime tag"),
            ExifError::Malformed(msg) => write!(f, "malformed EXIF data: {}", msg),
        }
    }
}

impl From<std::io::Error> for ExifError {
    fn from(e: std::io::Error) -> Self {
        ExifError::Io(e)
    }
}

/// Locates the TIFF-structured EXIF bytes in a file, however they're
/// wrapped. A JPEG carries them inside an APP1 segment; a bare TIFF or
/// TIFF-based RAW file (CR2, NEF, ORF, DNG, ...) *is* that structure from
/// byte zero, since they all share the same header format.
fn extract_exif_bytes(data: &[u8]) -> Result<&[u8], ExifError> {
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        jpeg::find_exif_segment(data)
    } else if data.len() >= 4 && (&data[0..2] == b"II" || &data[0..2] == b"MM") {
        Ok(data)
    } else {
        Err(ExifError::UnsupportedFormat)
    }
}

fn run(path: &str) -> Result<date::CaptureTime, ExifError> {
    let data = fs::read(path)?;
    let exif = extract_exif_bytes(&data)?;
    let raw = tiff::find_capture_time_raw(exif)?;
    date::parse(&raw.date, raw.subsec.as_deref(), raw.offset.as_deref()).map_err(ExifError::Malformed)
}

fn main() {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: exiftime <path-to-jpeg-or-tiff>");
            process::exit(2);
        }
    };

    match run(&path) {
        Ok(capture_time) => println!("{}", capture_time),
        Err(e) => {
            eprintln!("{}: {}", path, e);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_exif_bytes_dispatches_on_file_type() {
        let jpeg_no_exif = vec![0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x0C];
        assert!(matches!(
            extract_exif_bytes(&jpeg_no_exif),
            Err(ExifError::NoExif)
        ));

        let little_endian_tiff = vec![b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        assert!(matches!(extract_exif_bytes(&little_endian_tiff), Ok(_)));

        let big_endian_tiff = vec![b'M', b'M', 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        assert!(matches!(extract_exif_bytes(&big_endian_tiff), Ok(_)));

        let neither = vec![0x00, 0x01, 0x02, 0x03];
        assert!(matches!(
            extract_exif_bytes(&neither),
            Err(ExifError::UnsupportedFormat)
        ));

        let too_short = vec![b'I', b'I'];
        assert!(matches!(
            extract_exif_bytes(&too_short),
            Err(ExifError::UnsupportedFormat)
        ));
    }
}
