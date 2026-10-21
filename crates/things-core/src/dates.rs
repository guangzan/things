use chrono::{Datelike, Local, NaiveDate};

use crate::error::{Error, Result};

/// Decode Things' packed date (`year << 16 | month << 12 | day << 7`).
#[must_use]
pub fn decode_date(raw: i64) -> Option<NaiveDate> {
    if raw <= 0 {
        return None;
    }
    let year = i32::try_from(raw >> 16).ok()?;
    let month = u32::try_from((raw >> 12) & 0xF).ok()?;
    let day = u32::try_from((raw >> 7) & 0x1F).ok()?;
    NaiveDate::from_ymd_opt(year, month, day)
}

/// Encode a date exactly as Things stores it.
#[must_use]
pub fn encode_date(date: NaiveDate) -> i64 {
    let month = i64::from(date.month());
    let day = i64::from(date.day());
    (i64::from(date.year()) << 16) | (month << 12) | (day << 7)
}

/// Decode a packed reminder time (`hour << 26 | minute << 20`).
#[must_use]
pub fn decode_reminder(raw: i64) -> Option<(u32, u32)> {
    if raw < 0 {
        return None;
    }
    let hour = u32::try_from((raw >> 26) & 0x1F).ok()?;
    let minute = u32::try_from((raw >> 20) & 0x3F).ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// Parse ISO date or a Things keyword. The returned string is what should be
/// sent through the URL scheme.
#[must_use]
pub fn normalize_when(value: &str, today: NaiveDate) -> String {
    let trimmed = value.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "today" => today.format("%Y-%m-%d").to_string(),
        "tomorrow" => (today + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string(),
        "evening" => "evening".to_string(),
        "anytime" | "someday" => trimmed.to_ascii_lowercase(),
        _ => trimmed.to_string(),
    }
}

/// Parse a strict `HH:MM` reminder time and return its canonical form.
///
/// Things encodes reminder scheduling in the `when` URL parameter as
/// `date@HH:MM`; keeping the time here canonical avoids accepting values such
/// as `9:5` that Things would interpret differently or ignore.
pub fn parse_reminder(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.len() != 5
        || trimmed.as_bytes()[2] != b':'
        || !trimmed[..2].bytes().all(|byte| byte.is_ascii_digit())
        || !trimmed[3..].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::InvalidDate(format!(
            "{value:?} is not a valid reminder time; use HH:MM"
        )));
    }
    let time = chrono::NaiveTime::parse_from_str(trimmed, "%H:%M").map_err(|_| {
        Error::InvalidDate(format!("{value:?} is not a valid reminder time; use HH:MM"))
    })?;
    Ok(time.format("%H:%M").to_string())
}

/// Parse a Things `when` value that may carry a canonical reminder time.
pub fn parse_when_time(
    value: &str,
    today: NaiveDate,
) -> Result<(Option<NaiveDate>, Option<String>, bool)> {
    let (day, reminder) = value
        .split_once('@')
        .map_or((value, None), |(day, reminder)| (day, Some(reminder)));
    if day.contains('@') {
        return Err(Error::InvalidDate(format!(
            "{value:?} contains more than one reminder separator"
        )));
    }
    let reminder = reminder.map(parse_reminder).transpose()?;
    let normalized = normalize_when(day, today);
    match normalized.as_str() {
        "evening" => Ok((None, reminder, true)),
        "anytime" | "someday" => Err(Error::InvalidDate(format!(
            "{value:?} is a scheduling bucket, not a calendar date"
        ))),
        _ => parse_date(&normalized, today).map(|date| (Some(date), reminder, false)),
    }
}

/// Add a reminder time to a normalized Things scheduling value.
///
/// `fallback` is used when the caller did not supply `when`; for creates the
/// service uses today, while edits use the item's existing scheduled date so
/// setting a reminder cannot silently move it. `anytime` and `someday` have no
/// date to attach the reminder to, so they are rejected.
pub fn with_reminder(
    when: Option<String>,
    reminder: Option<&str>,
    fallback: Option<String>,
) -> Result<Option<String>> {
    let Some(reminder) = reminder else {
        return Ok(when);
    };
    let clock = parse_reminder(reminder)?;
    let base = when
        .filter(|value| !value.trim().is_empty())
        .or(fallback)
        .ok_or_else(|| Error::Conflict("reminder requires a scheduled date".to_string()))?;
    match base.to_ascii_lowercase().as_str() {
        "anytime" | "someday" => {
            return Err(Error::Conflict(format!(
                "reminder cannot be combined with {base:?}; schedule the item for a date, today, tomorrow, or evening first"
            )));
        }
        _ if base.contains('@') => {
            return Err(Error::Conflict(
                "when already contains a reminder time; use only one of when with @HH:MM or reminder"
                    .to_string(),
            ));
        }
        _ => {}
    }
    Ok(Some(format!("{base}@{clock}")))
}

/// Parse a strict ISO date or one of the scheduling keywords accepted by the
/// CLI. Unlike Things itself, invalid dates are rejected instead of being
/// silently reinterpreted.
pub fn parse_date(value: &str, today: NaiveDate) -> Result<NaiveDate> {
    let normalized = normalize_when(value, today);
    match normalized.as_str() {
        "evening" => Ok(today),
        "anytime" | "someday" => Err(Error::InvalidDate(format!(
            "{value:?} is a scheduling bucket, not a calendar date"
        ))),
        _ => NaiveDate::parse_from_str(&normalized, "%Y-%m-%d").map_err(|_| {
            Error::InvalidDate(format!(
                "{value:?} is not a valid date; use YYYY-MM-DD, today, or tomorrow"
            ))
        }),
    }
}

/// Convert a Things absolute timestamp (seconds since the Unix epoch).
#[must_use]
pub fn decode_timestamp(raw: f64) -> Option<chrono::DateTime<chrono::Utc>> {
    if raw <= 0.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let seconds = raw as i64;
    chrono::DateTime::from_timestamp(seconds, 0)
}

#[must_use]
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_round_trip() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        assert_eq!(decode_date(encode_date(date)), Some(date));
    }

    #[test]
    fn known_packed_date() {
        // 2026-10-19: 0x07EA1600 is not this date; compute the invariant.
        let date = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let raw = encode_date(date);
        assert_eq!(raw >> 16, 2026);
        assert_eq!((raw >> 12) & 0xF, 10);
        assert_eq!((raw >> 7) & 0x1F, 19);
    }

    #[test]
    fn reminder_round_trip() {
        let raw = (18_i64 << 26) | (45_i64 << 20);
        assert_eq!(decode_reminder(raw), Some((18, 45)));
        assert_eq!(decode_reminder(0), Some((0, 0)));
    }

    #[test]
    fn rejects_invalid_date() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        assert!(parse_date("2026-13-01", today).is_err());
    }

    #[test]
    fn reminder_time_is_strict_and_canonical() {
        assert_eq!(parse_reminder("09:05").unwrap(), "09:05");
        assert!(parse_reminder("9:5").is_err());
        assert!(parse_reminder("24:00").is_err());
    }

    #[test]
    fn reminder_folds_into_when_and_rejects_date_buckets() {
        assert_eq!(
            with_reminder(Some("2026-10-09".to_string()), Some("09:00"), None).unwrap(),
            Some("2026-10-09@09:00".to_string())
        );
        assert_eq!(
            with_reminder(None, Some("09:00"), Some("2026-10-09".to_string())).unwrap(),
            Some("2026-10-09@09:00".to_string())
        );
        assert_eq!(
            with_reminder(Some("evening".to_string()), Some("18:30"), None).unwrap(),
            Some("evening@18:30".to_string())
        );
        assert!(with_reminder(Some("anytime".to_string()), Some("09:00"), None).is_err());
        assert!(with_reminder(Some("someday".to_string()), Some("09:00"), None).is_err());
        assert!(with_reminder(Some("today@09:00".to_string()), Some("10:00"), None).is_err());
        assert_eq!(
            with_reminder(
                Some("  ".to_string()),
                Some("09:00"),
                Some("2026-10-09".to_string())
            )
            .unwrap(),
            Some("2026-10-09@09:00".to_string())
        );
    }

    #[test]
    fn reminder_schedule_parser_returns_date_time_and_evening_flag() {
        assert_eq!(
            parse_when_time(
                "2026-10-09@09:00",
                NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()
            )
            .unwrap(),
            (
                Some(NaiveDate::from_ymd_opt(2026, 10, 9).unwrap()),
                Some("09:00".to_string()),
                false
            )
        );
        assert_eq!(
            parse_when_time(
                "evening@18:30",
                NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()
            )
            .unwrap(),
            (None, Some("18:30".to_string()), true)
        );
    }
}
