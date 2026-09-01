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
}

impl fmt::Display for CaptureTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

/// Parses the raw `YYYY:MM:DD HH:MM:SS` string EXIF stores DateTimeOriginal
/// and DateTime as. Rejects the all-zero date some cameras write when they
/// have no clock set, and any field outside its calendar range, rather than
/// silently passing bad data through.
pub fn parse(raw: &str) -> Result<CaptureTime, String> {
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
                }),
            },
        ];

        for case in cases {
            let got = parse(case.input);
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
}
