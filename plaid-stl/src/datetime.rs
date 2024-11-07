use chrono::{NaiveDate, Utc, DateTime};
use serde::{Deserialize, Deserializer};

// Custom deserializer from a ISO 8601 string to a DateTime<Utc>
pub fn deserialize_rfc3339_timestamp<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    Ok(
        DateTime::parse_from_rfc3339(&s)
            .map_err(serde::de::Error::custom)?
            .with_timezone(&Utc)
    )
}

// Custom deserializer from a possibly empty ISO 8601 string to an optional DateTime<Utc>
pub fn deserialize_option_rfc3339_timestamp<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    // If the value is empty, we return None
    let s = match Option::<String>::deserialize(deserializer)? {
        Some(v) => v,
        None => return Ok(None),
    };

    let s = s.trim();
    // If the value is an empty string, we assume the default value has been returned instead of
    // throwing parsing errors
    if s.is_empty() {
        return Ok(None)
    }

    Ok(
        DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    )
}

// Deserialize a possibly empty string in "YYYY-MM-DD" format to an optional NaiveDate
pub fn deserialize_option_naivedate<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
where
    D: Deserializer<'de>,
{
    // If the value is empty, we return None
    let s = match Option::<String>::deserialize(deserializer)? {
        Some(v) => v,
        None => return Ok(None),
    };

    let s = s.trim();
    // If the value is an empty string, we assume the default value has been returned instead of
    // throwing parsing errors
    if s.is_empty() {
        return Ok(None)
    }

    // Parse the "YYYY-MM-DD" string into a NaiveDate
    Ok(Some(NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(serde::de::Error::custom)?))
}

