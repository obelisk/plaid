use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};

// Deserialize string in "YYYY-MM-DD" format
pub fn deserialize_naivedate_option<'de, D>(deserializer: D) -> Result<Option<NaiveDate>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    let s = s.trim();
    if s.is_empty() {
        return Ok(None)
    }

    // Parse the "YYYY-MM-DD" string into a NaiveDate
    Ok(Some(NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(serde::de::Error::custom)?))
}

