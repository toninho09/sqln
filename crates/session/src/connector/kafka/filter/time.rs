//! Times of Kafka expressions: `INTERVAL` texts and the formats of `TO_CHAR`. A time is a
//! number of milliseconds since 1970, in UTC, as the `timestamp` column.

use chrono::{DateTime, Datelike, Timelike, Utc};

use super::evaluation::Text;
use crate::connector::ExecError;
use crate::text;

/// The units of an interval, in milliseconds. Months and years have no fixed length.
const UNITS: [(&[&str], f64); 6] = [
    (&["ms", "msec", "msecs", "millisecond", "milliseconds"], 1.0),
    (&["s", "sec", "secs", "second", "seconds"], 1_000.0),
    (&["min", "mins", "minute", "minutes"], 60_000.0),
    (&["h", "hr", "hrs", "hour", "hours"], 3_600_000.0),
    (&["d", "day", "days"], 86_400_000.0),
    (&["w", "week", "weeks"], 604_800_000.0),
];

const VARIABLE_UNITS: [&str; 10] = [
    "m", "mon", "mons", "month", "months", "y", "yr", "yrs", "year", "years",
];

/// Reads an interval, such as `1 hour 30 minutes` or `90s`, as milliseconds.
pub fn interval(source: &str) -> Result<i64, &'static str> {
    let mut rest = source.trim_start();
    let mut total = 0.0;
    if rest.is_empty() {
        return Err(text::KAFKA_BAD_INTERVAL);
    }
    while !rest.is_empty() {
        let number_end = rest
            .char_indices()
            .find(|(index, char)| {
                !(char.is_ascii_digit() || *char == '.' || (*index == 0 && "+-".contains(*char)))
            })
            .map_or(rest.len(), |(index, _)| index);
        let amount: f64 = rest[..number_end]
            .parse()
            .map_err(|_| text::KAFKA_BAD_INTERVAL)?;
        rest = rest[number_end..].trim_start();
        let unit_end = rest
            .find(|char: char| !char.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let unit = &rest[..unit_end];
        rest = rest[unit_end..].trim_start();
        if VARIABLE_UNITS
            .iter()
            .any(|word| word.eq_ignore_ascii_case(unit))
        {
            return Err(text::KAFKA_INTERVAL_MONTH);
        }
        let factor = UNITS
            .iter()
            .find(|(words, _)| words.iter().any(|word| word.eq_ignore_ascii_case(unit)))
            .map(|(_, factor)| *factor)
            .ok_or(text::KAFKA_BAD_INTERVAL)?;
        total += amount * factor;
    }
    let total = total.round();
    if total.is_finite() && total.abs() < 9.0e18 {
        Ok(total as i64)
    } else {
        Err(text::KAFKA_BAD_INTERVAL)
    }
}

/// One part of a `TO_CHAR` format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Year,
    ShortYear,
    Month,
    Day,
    Hour24,
    Hour12,
    Minute,
    Second,
    Millisecond,
    Microsecond,
    Meridiem(Case),
    MonthName(Case, bool),
    DayName(Case, bool),
    Zone,
}

/// The case of a name, as the case of its pattern: `MONTH`, `Month`, or `month`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Title,
    Lower,
}

/// The patterns that ignore case, longest first.
const NUMBERS: [(&str, Piece); 10] = [
    ("HH24", Piece::Hour24),
    ("HH12", Piece::Hour12),
    ("YYYY", Piece::Year),
    ("HH", Piece::Hour12),
    ("YY", Piece::ShortYear),
    ("MM", Piece::Month),
    ("DD", Piece::Day),
    ("MI", Piece::Minute),
    ("SS", Piece::Second),
    ("MS", Piece::Millisecond),
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Reads the case of a name pattern, such as `Month`, at the start of the text.
fn name_case(text: &str, pattern: &str) -> Option<Case> {
    let found = text.get(..pattern.len())?;
    let title = format!("{}{}", &pattern[..1], pattern[1..].to_ascii_lowercase());
    [
        (pattern.to_owned(), Case::Upper),
        (title, Case::Title),
        (pattern.to_ascii_lowercase(), Case::Lower),
    ]
    .into_iter()
    .find(|(word, _)| *word == found)
    .map(|(_, case)| case)
}

/// Reads a `TO_CHAR` format. Text between double quotes and other characters stay the
/// same, as in Postgres.
pub fn format(source: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut rest = source;
    let mut literal = String::new();
    while let Some(char) = rest.chars().next() {
        if char == '"' {
            let inside = &rest[1..];
            let end = inside.find('"').unwrap_or(inside.len());
            literal.push_str(&inside[..end]);
            rest = inside.get(end + 1..).unwrap_or("");
            continue;
        }
        let named = [
            ("MONTH", true, true),
            ("MON", true, false),
            ("DAY", false, true),
            ("DY", false, false),
        ]
        .into_iter()
        .find_map(|(pattern, month, long)| {
            name_case(rest, pattern).map(|case| (pattern.len(), month, long, case))
        });
        let piece = if let Some((length, month, long, case)) = named {
            Some((
                length,
                if month {
                    Piece::MonthName(case, long)
                } else {
                    Piece::DayName(case, long)
                },
            ))
        } else if let Some(case) =
            ["AM", "PM"]
                .into_iter()
                .find_map(|pattern| match rest.get(..2) {
                    Some(found) if found == pattern => Some(Case::Upper),
                    Some(found) if found == pattern.to_ascii_lowercase() => Some(Case::Lower),
                    _ => None,
                })
        {
            Some((2, Piece::Meridiem(case)))
        } else if rest.starts_with("TZ") || rest.starts_with("tz") {
            Some((2, Piece::Zone))
        } else if rest
            .get(..2)
            .is_some_and(|found| found.eq_ignore_ascii_case("US"))
        {
            Some((2, Piece::Microsecond))
        } else {
            NUMBERS.iter().find_map(|(pattern, piece)| {
                rest.get(..pattern.len())
                    .filter(|found| found.eq_ignore_ascii_case(pattern))
                    .map(|_| (pattern.len(), piece.clone()))
            })
        };
        match piece {
            Some((length, piece)) => {
                if !literal.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut literal)));
                }
                pieces.push(piece);
                rest = &rest[length..];
            }
            None => {
                literal.push(char);
                rest = &rest[char.len_utf8()..];
            }
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Text(literal));
    }
    pieces
}

fn cased(name: &str, case: Case, long: bool) -> String {
    let name = if long { name } else { &name[..3] };
    match case {
        Case::Upper => name.to_ascii_uppercase(),
        Case::Title => name.to_owned(),
        Case::Lower => name.to_ascii_lowercase(),
    }
}

/// Writes a time with the parts of a format, in UTC.
pub fn write(pieces: &[Piece], time: DateTime<Utc>, out: &mut Text<'_>) -> Result<(), ExecError> {
    let (pm, hour12) = time.hour12();
    for piece in pieces {
        let part = match piece {
            Piece::Text(text) => {
                out.push(text.as_bytes())?;
                continue;
            }
            Piece::Year => format!("{:04}", time.year()),
            Piece::ShortYear => format!("{:02}", time.year().rem_euclid(100)),
            Piece::Month => format!("{:02}", time.month()),
            Piece::Day => format!("{:02}", time.day()),
            Piece::Hour24 => format!("{:02}", time.hour()),
            Piece::Hour12 => format!("{hour12:02}"),
            Piece::Minute => format!("{:02}", time.minute()),
            Piece::Second => format!("{:02}", time.second()),
            Piece::Millisecond => format!("{:03}", time.timestamp_subsec_millis()),
            Piece::Microsecond => format!("{:06}", time.timestamp_subsec_micros()),
            Piece::Meridiem(case) => {
                let word = if pm { "PM" } else { "AM" };
                match case {
                    Case::Lower => word.to_ascii_lowercase(),
                    _ => word.to_owned(),
                }
            }
            Piece::MonthName(case, long) => cased(MONTHS[time.month0() as usize], *case, *long),
            Piece::DayName(case, long) => cased(
                DAYS[time.weekday().num_days_from_monday() as usize],
                *case,
                *long,
            ),
            Piece::Zone => "UTC".to_owned(),
        };
        out.push(part.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connector::kafka::filter::Evaluation;

    #[test]
    fn an_interval_reads_numbers_and_units() {
        assert_eq!(interval("1 hour"), Ok(3_600_000));
        assert_eq!(interval("1 hour 30 minutes"), Ok(5_400_000));
        assert_eq!(interval("90s"), Ok(90_000));
        assert_eq!(interval("1.5 SECONDS"), Ok(1_500));
        assert_eq!(interval("-2 d"), Ok(-172_800_000));
        assert_eq!(interval("250 ms 1 week"), Ok(604_800_250));
        assert_eq!(interval("1 month"), Err(text::KAFKA_INTERVAL_MONTH));
        assert_eq!(interval("3 m"), Err(text::KAFKA_INTERVAL_MONTH));
        for bad in ["", "hour", "1", "1 hours x", "1 fortnight", "1..2 s"] {
            assert_eq!(interval(bad), Err(text::KAFKA_BAD_INTERVAL), "{bad}");
        }
    }

    fn formatted(format_text: &str, millis: i64) -> String {
        let evaluation = Evaluation::default();
        let mut out = Text::new(&evaluation).unwrap();
        let time = DateTime::from_timestamp_millis(millis).unwrap();
        write(&format(format_text), time, &mut out).unwrap();
        out.as_str().to_owned()
    }

    #[test]
    fn a_format_writes_the_parts_of_a_utc_time() {
        // 2026-11-03T14:05:09.123Z is a Tuesday.
        let millis = 1_793_714_709_123;
        assert_eq!(
            formatted("YYYY-MM-DD HH24:MI:SS.MS", millis),
            "2026-11-03 14:05:09.123"
        );
        assert_eq!(formatted("yyyy/mm/dd", millis), "2026/11/03");
        assert_eq!(formatted("HH12 AM pm HH", millis), "02 PM pm 02");
        assert_eq!(
            formatted("Month MON mon Dy DAY", millis),
            "November NOV nov Tue TUESDAY"
        );
        assert_eq!(formatted("YY US TZ", millis), "26 123000 UTC");
        assert_eq!(
            formatted(r#"YYYY-MM-DD"T"HH24 "at" "DD""#, millis),
            "2026-11-03T14 at DD"
        );
        assert_eq!(formatted("Q # é", millis), "Q # é");
    }
}
