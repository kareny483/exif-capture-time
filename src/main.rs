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
    NotJpeg,
    NoExif,
    NoDateTag,
    Malformed(String),
}

impl fmt::Display for ExifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExifError::Io(e) => write!(f, "could not read file: {}", e),
            ExifError::NotJpeg => write!(f, "not a JPEG file (missing SOI marker)"),
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

fn run(path: &str) -> Result<date::CaptureTime, ExifError> {
    let data = fs::read(path)?;
    let exif = jpeg::find_exif_segment(&data)?;
    let raw = tiff::find_capture_time_raw(exif)?;
    date::parse(&raw).map_err(ExifError::Malformed)
}

fn main() {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: exiftime <path-to-jpeg>");
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
