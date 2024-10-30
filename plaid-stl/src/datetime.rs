use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

// Custom deserializer for an optional DateTime<Utc>
pub fn deserialize_option_timestamp<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    // Try to deserialize as a string (ISO 8601 format) or return None if missing
    let opt = Option::<String>::deserialize(deserializer)?;

    // Parse the timestamp string into a DateTime<Utc>, if it's present
    Ok(opt.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    }))
}
