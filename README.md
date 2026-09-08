# exiftime

Answers one question: what timestamp does a JPEG's EXIF data say the photo
was taken at?

Every photo library eventually runs into files whose "date taken" is wrong,
missing, or disagrees with the filesystem timestamp. Before sorting that out
you need something that shows you exactly what's stored in the file, not
whatever your OS or photo app decided to display after guessing around
missing or malformed dates. `exiftime` reads the DateTimeOriginal tag
straight out of the EXIF block and prints it, or explains precisely why it
couldn't (no EXIF segment, no date tag, the all-zero placeholder some
cameras write for "unknown", an out-of-range field, and so on).

## Usage

    cargo run --release -- path/to/photo.jpg

Output on success is the capture time in `YYYY-MM-DD HH:MM:SS` form:

    2023-07-04 14:22:09

When the file also carries SubSecTimeOriginal and/or OffsetTimeOriginal (or
their non-"Original" counterparts, if that's what the date came from), the
fractional seconds and UTC offset are appended:

    2023-07-04 14:22:09.500-07:00

A malformed subsecond or offset value is dropped rather than treated as a
failure: DateTimeOriginal is what answers the question, the other two just
sharpen it.

On failure it prints the reason to stderr and exits non-zero:

    photo.jpg: EXIF data has no DateTimeOriginal or DateTime tag

## How it works

1. Scan the JPEG's marker segments for the APP1 segment carrying an
   `Exif\0\0` header (`src/jpeg.rs`).
2. Parse the TIFF structure inside that segment: byte order, IFD0, and the
   Exif sub-IFD, to find tag `0x9003` (DateTimeOriginal), falling back to
   `0x0132` (DateTime) on IFD0 if the more specific tag is absent. Also reads
   whichever of SubSecTime(Original) and OffsetTime(Original) match the date
   tag that was actually used (`src/tiff.rs`).
3. Parse and validate the raw `YYYY:MM:DD HH:MM:SS` string, rejecting the
   all-zero "unknown date" placeholder and out-of-range fields, and parse
   the subsecond/offset strings if present (`src/date.rs`).

## Scope

JPEG only, for now. No dependencies: the parsing involved is small enough
that pulling in an image or EXIF crate would cost more in trust and compile
time than it saves.

## Status

Early. See the test suite in `src/date.rs` for the set of malformed-date
cases it's already expected to handle correctly, `src/jpeg.rs` for the
segment-scanning edge cases, and `src/tiff.rs` for malformed TIFF/IFD cases
(bad byte order markers, out-of-range offsets, truncated entry tables).
