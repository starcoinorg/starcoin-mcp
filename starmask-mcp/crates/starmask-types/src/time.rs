use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd, Hash,
)]
#[serde(transparent)]
pub struct DurationSeconds(u64);

impl DurationSeconds {
    pub const fn new(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn as_secs(self) -> u64 {
        self.0
    }
}

impl Display for DurationSeconds {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}s", self.0)
    }
}

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd, Hash,
)]
#[serde(transparent)]
pub struct TimestampMs(i64);

impl TimestampMs {
    pub const fn from_millis(value: i64) -> Self {
        Self(value)
    }

    pub const fn as_millis(self) -> i64 {
        self.0
    }

    pub fn checked_add_seconds(self, duration: DurationSeconds) -> Option<Self> {
        let millis = duration.as_secs().checked_mul(1000)?;
        let millis = i64::try_from(millis).ok()?;
        self.0.checked_add(millis).map(Self)
    }
}

impl Display for TimestampMs {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
