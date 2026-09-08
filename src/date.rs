use std::fmt;

/// A validated EXIF capture time. Deliberately not tied to any calendar
/// library: EXIF timestamps have no timezone by default and only need to be
/// displayed back out, not compared or arithmetic'd on.
#[derive(Debug, PartialEq, Eq)]
pub struct CaptureTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub subsec: Option<String>,
    pub offset: Option<Offset>,
}

/// A UTC offset from OffsetTime/OffsetTimeOriginal, e.g. `-07:00`.
#[derive(Debug, PartialEq, Eq)]
pub struct Offset {
    pub negative: bool,
    pub hours: u8,
    pub minutes: u8,
}

impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{:02}:{:02}",
            if self.negative { "-" } else { "+" },
            self.hours,
            self.minutes
        )
    }
}

impl fmt::Display for CaptureTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )?;
        if let Some(subsec) = &self.subsec {
            write!(f, ".{}", subsec)?;
        }
        if let Some(offset) = &self.offset {
            write!(f, "{}", offset)?;
        }
        Ok(())
    }
}

/// Parses the raw `YYYY:MM:DD HH:MM:SS` string EXIF stores DateTimeOriginal
/// and DateTime as, plus the optional SubSecTime(Original) and
/// OffsetTime(Original) strings that accompany it. Rejects the all-zero date
/// some cameras write when they have no clock set, and any date/time field
/// outside its calendar range, rather than silently passing bad data
/// through. A malformed subsecond or offset value is dropped rather than
/// failing the whole parse: they're supplementary precision, not required
/// to answer "when was this taken".
pub fn parse(raw: &str, subsec: Option<&str>, offset: Option<&str>) -> Result<CaptureTime, String> {
    let bytes = raw.as_bytes();
    if bytes.len() != 19 {
        return Err(format!("expected 19 characters, got {}", bytes.len()));
    }
    if bytes[4] != b':' || bytes[7] != b':' || bytes[10] != b' ' || bytes[13] != b':' || bytes[16] != b':' {
        return Err("does not match YYYY:MM:DD HH:MM:SS layout".to_string());
    }

    let year = parse_field(&raw[0..4])?;
    let month = parse_field(&raw[5..7])?;
    let day = parse_field(&raw[8..10])?;
    let hour = parse_field(&raw[11..13])?;
    let minute = parse_field(&raw[14..16])?;
    let second = parse_field(&raw[17..19])?;

    if year == 0 && month == 0 && day == 0 {
        return Err("camera recorded an unknown date (all zero)".to_string());
    }
    if !(1..=12).contains(&month) {
        return Err(format!("month {} out of range", month));
    }
    if !(1..=31).contains(&day) {
        return Err(format!("day {} out of range", day));
    }
    if hour > 23 {
        return Err(format!("hour {} out of range", hour));
    }
    if minute > 59 {
        return Err(format!("minute {} out of range", minute));
    }
    if second > 60 {
        // 60 is allowed: some encoders record a leap second this way.
        return Err(format!("second {} out of range", second));
    }

    Ok(CaptureTime {
        year: year as u16,
        month: month as u8,
        day: day as u8,
        hour: hour as u8,
        minute: minute as u8,
        second: second as u8,
        subsec: subsec.and_then(valid_subsec),
        offset: offset.and_then(|s| parse_offset(s).ok()),
    })
}

/// EXIF's subsecond tags are a decimal fraction stored as digit characters
/// (e.g. "500" means .500), not a fixed-width field, so any nonempty numeric
/// string is accepted as-is.
fn valid_subsec(raw: &str) -> Option<String> {
    if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        Some(raw.to_string())
    } else {
        None
    }
}

/// Parses an OffsetTime/OffsetTimeOriginal value: `+HH:MM`, `-HH:MM`, or the
/// nonstandard but occasionally seen `Z` for UTC.
fn parse_offset(raw: &str) -> Result<Offset, String> {
    if raw.eq_ignore_ascii_case("z") {
        return Ok(Offset {
            negative: false,
            hours: 0,
            minutes: 0,
        });
    }

    let bytes = raw.as_bytes();
    if bytes.len() != 6 || bytes[3] != b':' {
        return Err(format!("'{}' does not match +HH:MM layout", raw));
    }
    let negative = match bytes[0] {
        b'+' => false,
        b'-' => true,
        _ => return Err(format!("'{}' has no leading sign", raw)),
    };

    let hours = parse_field(&raw[1..3])?;
    let minutes = parse_field(&raw[4..6])?;
    if hours > 14 {
        return Err(format!("offset hours {} out of range", hours));
    }
    if minutes > 59 {
        return Err(format!("offset minutes {} out of range", minutes));
    }

    Ok(Offset {
        negative,
        hours: hours as u8,
        minutes: minutes as u8,
    })
}

fn parse_field(s: &str) -> Result<u32, String> {
    s.parse::<u32>().map_err(|_| format!("'{}' is not a number", s))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        name: &'static str,
        input: &'static str,
        want_ok: Option<CaptureTime>,
    }

    #[test]
    fn parse_cases() {
        let cases = vec![
            Case {
                name: "well formed",
                input: "2023:07:04 14:22:09",
                want_ok: Some(CaptureTime {
                    year: 2023,
                    month: 7,
                    day: 4,
                    hour: 14,
                    minute: 22,
                    second: 9,
                    subsec: None,
                    offset: None,
                }),
            },
            Case {
                name: "all zero means camera has no clock set",
                input: "0000:00:00 00:00:00",
                want_ok: None,
            },
            Case {
                name: "leap second is accepted",
                input: "2016:12:31 23:59:60",
                want_ok: Some(CaptureTime {
                    year: 2016,
                    month: 12,
                    day: 31,
                    hour: 23,
                    minute: 59,
                    second: 60,
                    subsec: None,
                    offset: None,
                }),
            },
            Case {
                name: "month out of range",
                input: "2023:13:01 00:00:01",
                want_ok: None,
            },
            Case {
                name: "day zero with nonzero year is not the unknown-date case",
                input: "2023:07:00 00:00:00",
                want_ok: None,
            },
            Case {
                name: "hour out of range",
                input: "2023:07:04 24:00:00",
                want_ok: None,
            },
            Case {
                name: "minute out of range",
                input: "2023:07:04 00:60:00",
                want_ok: None,
            },
            Case {
                name: "second past leap-second allowance",
                input: "2023:07:04 00:00:61",
                want_ok: None,
            },
            Case {
                name: "too short",
                input: "2023:07:04 14:22",
                want_ok: None,
            },
            Case {
                name: "too long",
                input: "2023:07:04 14:22:09Z",
                want_ok: None,
            },
            Case {
                name: "wrong separators",
                input: "2023-07-04 14:22:09",
                want_ok: None,
            },
            Case {
                name: "non numeric field",
                input: "202a:07:04 14:22:09",
                want_ok: None,
            },
            Case {
                name: "midnight new year is valid",
                input: "2000:01:01 00:00:00",
                want_ok: Some(CaptureTime {
                    year: 2000,
                    month: 1,
                    day: 1,
                    hour: 0,
                    minute: 0,
                    second: 0,
                    subsec: None,
                    offset: None,
                }),
            },
        ];

        for case in cases {
            let got = parse(case.input, None, None);
            match (&case.want_ok, &got) {
                (Some(want), Ok(got)) => {
                    assert_eq!(want, got, "case '{}'", case.name)
                }
                (None, Err(_)) => {}
                _ => panic!(
                    "case '{}': want {:?}, got {:?}",
                    case.name, case.want_ok, got
                ),
            }
        }
    }

    #[test]
    fn subsec_is_included_when_present_and_numeric() {
        let got = parse("2023:07:04 14:22:09", Some("500"), None).unwrap();
        assert_eq!(got.subsec.as_deref(), Some("500"));
        assert_eq!(got.to_string(), "2023-07-04 14:22:09.500");
    }

    #[test]
    fn non_numeric_subsec_is_dropped_rather_than_failing_the_parse() {
        let got = parse("2023:07:04 14:22:09", Some("abc"), None).unwrap();
        assert_eq!(got.subsec, None);
    }

    #[test]
    fn positive_offset_is_included() {
        let got = parse("2023:07:04 14:22:09", None, Some("+05:30")).unwrap();
        assert_eq!(got.to_string(), "2023-07-04 14:22:09+05:30");
    }

    #[test]
    fn negative_offset_is_included() {
        let got = parse("2023:07:04 14:22:09", None, Some("-07:00")).unwrap();
        assert_eq!(got.to_string(), "2023-07-04 14:22:09-07:00");
    }

    #[test]
    fn subsec_and_offset_combine_in_display() {
        let got = parse("2023:07:04 14:22:09", Some("12"), Some("+00:00")).unwrap();
        assert_eq!(got.to_string(), "2023-07-04 14:22:09.12+00:00");
    }

    #[test]
    fn z_offset_means_utc() {
        let got = parse("2023:07:04 14:22:09", None, Some("Z")).unwrap();
        assert_eq!(got.to_string(), "2023-07-04 14:22:09+00:00");
    }

    #[test]
    fn malformed_offset_is_dropped_rather_than_failing_the_parse() {
        let got = parse("2023:07:04 14:22:09", None, Some("not-an-offset")).unwrap();
        assert_eq!(got.offset, None);
    }

    #[test]
    fn offset_hours_out_of_range_is_dropped() {
        let got = parse("2023:07:04 14:22:09", None, Some("+15:00")).unwrap();
        assert_eq!(got.offset, None);
    }
}
