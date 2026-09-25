use std::env;
use std::fmt;
use std::fs;
use std::path::Path;
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

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "tif", "tiff", "cr2", "nef", "orf", "dng",
];

fn has_image_extension(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => IMAGE_EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(ext)),
        None => false,
    }
}

/// Collects the paths of every file directly inside `dir` whose extension
/// matches one of the formats this tool understands, in sorted order for
/// stable output.
fn collect_batch_paths(dir: &str) -> Result<Vec<String>, std::io::Error> {
    let mut paths: Vec<String> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.is_file() && has_image_extension(p))
        .filter_map(|p| p.to_str().map(String::from))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Minimal escaping for the handful of characters that can actually show up
/// in our output: a file path (in error JSON) or the parts of a CaptureTime.
/// Not a general-purpose JSON string encoder.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn capture_time_to_json(t: &date::CaptureTime) -> String {
    let mut fields = format!(
        "\"timestamp\":\"{}\",\"year\":{},\"month\":{},\"day\":{},\"hour\":{},\"minute\":{},\"second\":{}",
        json_escape(&t.to_string()),
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second
    );
    fields.push_str(",\"subsec\":");
    match &t.subsec {
        Some(s) => fields.push_str(&format!("\"{}\"", json_escape(s))),
        None => fields.push_str("null"),
    }
    fields.push_str(",\"offset\":");
    match &t.offset {
        Some(o) => fields.push_str(&format!("\"{}\"", json_escape(&o.to_string()))),
        None => fields.push_str("null"),
    }
    format!("{{{}}}", fields)
}

fn print_result(path: &str, result: &Result<date::CaptureTime, ExifError>, json: bool) {
    if json {
        let body = match result {
            Ok(t) => format!("\"ok\":true,\"result\":{}", capture_time_to_json(t)),
            Err(e) => format!(
                "\"ok\":false,\"error\":\"{}\"",
                json_escape(&e.to_string())
            ),
        };
        println!(
            "{{\"path\":\"{}\",{}}}",
            json_escape(path),
            body
        );
    } else {
        match result {
            Ok(t) => println!("{}", t),
            Err(e) => eprintln!("{}: {}", path, e),
        }
    }
}

fn main() {
    let mut json = false;
    let mut path = None;
    for arg in env::args().skip(1) {
        if arg == "--json" {
            json = true;
        } else if path.is_none() {
            path = Some(arg);
        } else {
            eprintln!("usage: exiftime [--json] <path-to-jpeg-or-tiff-or-directory>");
            process::exit(2);
        }
    }
    let path = match path {
        Some(p) => p,
        None => {
            eprintln!("usage: exiftime [--json] <path-to-jpeg-or-tiff-or-directory>");
            process::exit(2);
        }
    };

    if Path::new(&path).is_dir() {
        let paths = match collect_batch_paths(&path) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}: could not read directory: {}", path, e);
                process::exit(1);
            }
        };
        if paths.is_empty() {
            eprintln!("{}: no JPEG or TIFF files found", path);
            process::exit(1);
        }
        let mut any_failed = false;
        for file_path in &paths {
            let result = run(file_path);
            any_failed |= result.is_err();
            print_result(file_path, &result, json);
        }
        if any_failed {
            process::exit(1);
        }
        return;
    }

    let result = run(&path);
    let failed = result.is_err();
    print_result(&path, &result, json);
    if failed {
        process::exit(1);
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

    #[test]
    fn json_escape_handles_quotes_backslashes_and_control_chars() {
        assert_eq!(json_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(json_escape("\u{7}"), "\\u0007");
        assert_eq!(json_escape("plain"), "plain");
    }

    #[test]
    fn capture_time_to_json_includes_null_subsec_and_offset_when_absent() {
        let t = date::parse("2023:07:04 14:22:09", None, None).unwrap();
        assert_eq!(
            capture_time_to_json(&t),
            "{\"timestamp\":\"2023-07-04 14:22:09\",\"year\":2023,\"month\":7,\"day\":4,\
             \"hour\":14,\"minute\":22,\"second\":9,\"subsec\":null,\"offset\":null}"
        );
    }

    #[test]
    fn capture_time_to_json_includes_subsec_and_offset_when_present() {
        let t = date::parse("2023:07:04 14:22:09", Some("500"), Some("-07:00")).unwrap();
        let json = capture_time_to_json(&t);
        assert!(json.contains("\"subsec\":\"500\""));
        assert!(json.contains("\"offset\":\"-07:00\""));
    }

    #[test]
    fn has_image_extension_matches_known_formats_case_insensitively() {
        assert!(has_image_extension(Path::new("photo.jpg")));
        assert!(has_image_extension(Path::new("photo.JPEG")));
        assert!(has_image_extension(Path::new("photo.CR2")));
        assert!(!has_image_extension(Path::new("photo.png")));
        assert!(!has_image_extension(Path::new("noextension")));
    }

    /// Builds a scratch directory under the OS temp dir, unique to this
    /// test run, so parallel test threads don't collide.
    fn make_scratch_dir(name: &str) -> std::path::PathBuf {
        let mut dir = env::temp_dir();
        dir.push(format!("exiftime-test-{}-{}", name, process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn collect_batch_paths_filters_to_known_extensions_and_sorts() {
        let dir = make_scratch_dir("collect-batch-paths");
        fs::write(dir.join("b.jpg"), b"").unwrap();
        fs::write(dir.join("a.tif"), b"").unwrap();
        fs::write(dir.join("notes.txt"), b"").unwrap();
        fs::create_dir_all(dir.join("subdir.jpg")).unwrap();

        let found = collect_batch_paths(dir.to_str().unwrap()).unwrap();
        let names: Vec<&str> = found
            .iter()
            .map(|p| Path::new(p).file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.tif", "b.jpg"]);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_batch_paths_errors_on_missing_directory() {
        assert!(collect_batch_paths("/no/such/exiftime/test/dir").is_err());
    }
}
