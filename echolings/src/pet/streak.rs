//! Days together and the streak, counted in the user's local days.

use serde::{Deserialize, Serialize};

use super::balance::REST_DAY_EVERY;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Streak {
    /// Days in a row (rest days are bridged, not counted).
    pub count: u32,
    /// All days the user spent with the pet.
    pub days_together: u32,
    pub last_day: Option<i64>,
    /// The last missed day that was bridged.
    pub rest_day: Option<i64>,
}

impl Streak {
    /// Records that the user spent `day` with the pet. Calling it again on the same day does
    /// nothing; a day in the past (e.g. the clock moved back) is ignored.
    pub fn mark(&mut self, day: i64) {
        match self.last_day {
            Some(last) if day <= last => return,
            Some(last) if day == last + 1 => self.count += 1,
            Some(last) if day == last + 2 && self.rest_available(last + 1) => {
                self.count += 1;
                self.rest_day = Some(last + 1);
            }
            _ => self.count = 1,
        }
        self.last_day = Some(day);
        self.days_together += 1;
    }

    /// The streak as it stands on `today`: zero once it can no longer be continued.
    pub fn current(&self, today: i64) -> u32 {
        match self.last_day {
            Some(last) if today - last <= 1 => self.count,
            Some(last) if today - last == 2 && self.rest_available(last + 1) => self.count,
            _ => 0,
        }
    }

    fn rest_available(&self, missed: i64) -> bool {
        self.rest_day
            .is_none_or(|rest| missed - rest >= REST_DAY_EVERY)
    }
}
