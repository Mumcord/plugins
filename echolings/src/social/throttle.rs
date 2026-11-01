//! When to send the pet card: on joining a channel, when someone joins, and when it changes,
//! but at most once per minute (FR-6.83).

pub const CARD_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Everyone has the current card.
    Idle,
    Send,
    /// Ask again in this many seconds.
    Wait(u64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardThrottle {
    last_attempt: Option<u64>,
    /// What the channel last received; None = someone may not have the card yet.
    delivered: Option<Vec<u8>>,
}

impl CardThrottle {
    /// Someone new may not know the card (the user moved, or someone joined): send it again
    /// even if it did not change.
    pub fn invalidate(&mut self) {
        self.delivered = None;
    }

    pub fn decide(&self, card: &[u8], now: u64) -> Decision {
        if self.delivered.as_deref() == Some(card) {
            return Decision::Idle;
        }
        match self.last_attempt {
            Some(last) if now < last + CARD_INTERVAL_SECS => {
                Decision::Wait(last + CARD_INTERVAL_SECS - now)
            }
            _ => Decision::Send,
        }
    }

    /// Records a send attempt. Failed attempts count towards the limit too, so a refusing host
    /// is asked at most once a minute; only a delivered card stops further sends.
    pub fn attempted(&mut self, card: &[u8], delivered: bool, now: u64) {
        self.last_attempt = Some(now);
        if delivered {
            self.delivered = Some(card.to_vec());
        }
    }
}
