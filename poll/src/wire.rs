//! The Poll message formats (FR-6.81). Payloads are compact binary rather than JSON: escaping
//! untrusted text in JSON can grow a full poll beyond the 997-byte message limit.
//!
//! All integers are big-endian; strings are a one-byte length followed by UTF-8 bytes. Decoders
//! treat every payload as untrusted: wrong lengths, invalid UTF-8, control characters or
//! trailing bytes reject the whole message.

/// Kind 1: the creator announces a poll to the channel.
pub const KIND_CREATE: u8 = 1;
/// Kind 2: a voter tells the creator (directly) which option they chose.
pub const KIND_VOTE: u8 = 2;
/// Kind 3: the creator tells the channel the current counts.
pub const KIND_TALLY: u8 = 3;
/// Kind 4: the creator tells the channel who voted for what.
pub const KIND_VOTERS: u8 = 4;

pub const MAX_QUESTION_BYTES: usize = 200;
pub const MAX_OPTION_BYTES: usize = 40;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 10;
/// Names in the voter list are cut to this many bytes so up to 100 voters fit in a few messages.
pub const MAX_VOTER_NAME_BYTES: usize = 16;
pub const MAX_VOTERS: usize = 100;
/// The payload limit of a plugin message (FR-6.34).
pub const MAX_PAYLOAD: usize = 997;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Create {
    pub poll_id: u32,
    /// Hash of the fallback chat line (`chat.line-hash`), 0 if no line was posted.
    pub line_hash: u32,
    pub question: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vote {
    pub poll_id: u32,
    pub option: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    pub poll_id: u32,
    pub closed: bool,
    /// Hash of the closing chat line, 0 if none.
    pub close_hash: u32,
    pub counts: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoterEntry {
    pub option: u8,
    pub name: String,
}

/// One part of the voter list; a long list is split into several parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voters {
    pub poll_id: u32,
    pub part: u8,
    pub parts: u8,
    pub entries: Vec<VoterEntry>,
}

/// Checks the question and options a user typed or a peer sent.
pub fn valid_poll(question: &str, options: &[String]) -> bool {
    valid_text(question, MAX_QUESTION_BYTES)
        && (MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len())
        && options.iter().all(|o| valid_text(o, MAX_OPTION_BYTES))
}

fn valid_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

/// Cuts `text` to at most `max` bytes on a character boundary.
pub fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl Create {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MAX_PAYLOAD);
        out.extend_from_slice(&self.poll_id.to_be_bytes());
        out.extend_from_slice(&self.line_hash.to_be_bytes());
        put_str(&mut out, &self.question);
        out.push(len_u8(self.options.len()));
        for option in &self.options {
            put_str(&mut out, option);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let poll_id = r.u32()?;
        let line_hash = r.u32()?;
        let question = r.str(MAX_QUESTION_BYTES)?;
        let count = usize::from(r.u8()?);
        if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&count) {
            return None;
        }
        let options = (0..count)
            .map(|_| r.str(MAX_OPTION_BYTES))
            .collect::<Option<Vec<_>>>()?;
        r.finish()?;
        valid_poll(&question, &options).then_some(Self {
            poll_id,
            line_hash,
            question,
            options,
        })
    }
}

impl Vote {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.poll_id.to_be_bytes().to_vec();
        out.push(self.option);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let vote = Self {
            poll_id: r.u32()?,
            option: r.u8()?,
        };
        r.finish()?;
        Some(vote)
    }
}

impl Tally {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.poll_id.to_be_bytes().to_vec();
        out.push(u8::from(self.closed));
        out.extend_from_slice(&self.close_hash.to_be_bytes());
        out.push(len_u8(self.counts.len()));
        for count in &self.counts {
            out.extend_from_slice(&count.to_be_bytes());
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let poll_id = r.u32()?;
        let closed = match r.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let close_hash = r.u32()?;
        let count = usize::from(r.u8()?);
        if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&count) {
            return None;
        }
        let counts = (0..count).map(|_| r.u16()).collect::<Option<Vec<_>>>()?;
        r.finish()?;
        Some(Self {
            poll_id,
            closed,
            close_hash,
            counts,
        })
    }
}

/// Bytes before the entries of a voter-list part: poll id, part, parts, entry count.
const VOTERS_HEADER: usize = 7;

impl Voters {
    /// Splits a voter list into parts that each fit one message. At most [`MAX_VOTERS`] entries
    /// are sent; names are cut to [`MAX_VOTER_NAME_BYTES`]. An empty list still gives one part,
    /// so receivers learn that nobody voted.
    pub fn split(poll_id: u32, entries: &[VoterEntry]) -> Vec<Self> {
        let mut parts: Vec<Vec<VoterEntry>> = vec![Vec::new()];
        let mut size = VOTERS_HEADER;
        for entry in entries.iter().take(MAX_VOTERS) {
            let name = truncate(&entry.name, MAX_VOTER_NAME_BYTES).to_owned();
            let entry_size = 2 + name.len();
            if size + entry_size > MAX_PAYLOAD {
                parts.push(Vec::new());
                size = VOTERS_HEADER;
            }
            size += entry_size;
            if let Some(last) = parts.last_mut() {
                last.push(VoterEntry {
                    option: entry.option,
                    name,
                });
            }
        }
        let total = len_u8(parts.len());
        parts
            .into_iter()
            .enumerate()
            .map(|(i, entries)| Self {
                poll_id,
                part: len_u8(i),
                parts: total,
                entries,
            })
            .collect()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.poll_id.to_be_bytes().to_vec();
        out.push(self.part);
        out.push(self.parts);
        out.push(len_u8(self.entries.len()));
        for entry in &self.entries {
            out.push(entry.option);
            put_str(&mut out, &entry.name);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let poll_id = r.u32()?;
        let part = r.u8()?;
        let parts = r.u8()?;
        if parts == 0 || part >= parts {
            return None;
        }
        let count = usize::from(r.u8()?);
        if count > MAX_VOTERS {
            return None;
        }
        let entries = (0..count)
            .map(|_| {
                let option = r.u8()?;
                let name = r.str(MAX_VOTER_NAME_BYTES)?;
                (usize::from(option) < MAX_OPTIONS && !name.chars().any(char::is_control))
                    .then_some(VoterEntry { option, name })
            })
            .collect::<Option<Vec<_>>>()?;
        r.finish()?;
        Some(Self {
            poll_id,
            part,
            parts,
            entries,
        })
    }
}

/// All lengths written here are bounded far below 256 by the validation above.
fn len_u8(len: usize) -> u8 {
    u8::try_from(len).unwrap_or(u8::MAX)
}

fn put_str(out: &mut Vec<u8>, text: &str) {
    let text = truncate(text, usize::from(u8::MAX));
    out.push(len_u8(text.len()));
    out.extend_from_slice(text.as_bytes());
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2)?.try_into().ok().map(u16::from_be_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_be_bytes)
    }

    fn str(&mut self, max: usize) -> Option<String> {
        let len = usize::from(self.u8()?);
        if len > max {
            return None;
        }
        std::str::from_utf8(self.take(len)?).ok().map(str::to_owned)
    }

    fn finish(&self) -> Option<()> {
        self.0.is_empty().then_some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create() -> Create {
        Create {
            poll_id: 7,
            line_hash: 0xdead_beef,
            question: "Pizza or pasta?".into(),
            options: vec!["Pizza".into(), "Pasta".into()],
        }
    }

    #[test]
    fn create_round_trips() {
        let c = create();
        assert_eq!(Create::decode(&c.encode()), Some(c));
    }

    #[test]
    fn largest_poll_fits_one_message() {
        let c = Create {
            poll_id: u32::MAX,
            line_hash: u32::MAX,
            question: "ä".repeat(100),
            options: vec!["ö".repeat(20); MAX_OPTIONS],
        };
        let bytes = c.encode();
        assert!(bytes.len() <= MAX_PAYLOAD, "{} bytes", bytes.len());
        assert_eq!(Create::decode(&bytes), Some(c));
    }

    #[test]
    fn create_rejects_bad_input() {
        let good = create().encode();
        // Truncated anywhere or with trailing bytes.
        for len in 0..good.len() {
            assert_eq!(Create::decode(&good[..len]), None, "prefix {len}");
        }
        let mut long = good.clone();
        long.push(0);
        assert_eq!(Create::decode(&long), None);

        let mut c = create();
        c.options = vec!["only".into()];
        assert_eq!(Create::decode(&c.encode()), None, "one option");
        c.options = vec!["x".into(); MAX_OPTIONS + 1];
        assert_eq!(Create::decode(&c.encode()), None, "eleven options");

        let mut c = create();
        c.question = "q".repeat(MAX_QUESTION_BYTES + 1);
        assert_eq!(Create::decode(&c.encode()), None, "long question");
        let mut c = create();
        c.options[0] = "o".repeat(MAX_OPTION_BYTES + 1);
        assert_eq!(Create::decode(&c.encode()), None, "long option");
        let mut c = create();
        c.question = "line\nbreak".into();
        assert_eq!(Create::decode(&c.encode()), None, "control character");
        let mut c = create();
        c.options[1] = "   ".into();
        assert_eq!(Create::decode(&c.encode()), None, "blank option");

        let mut bad_utf8 = good;
        bad_utf8[9] = 0xff;
        assert_eq!(Create::decode(&bad_utf8), None, "invalid UTF-8");
    }

    #[test]
    fn vote_round_trips_and_rejects_bad_length() {
        let v = Vote {
            poll_id: 3,
            option: 1,
        };
        assert_eq!(Vote::decode(&v.encode()), Some(v));
        assert_eq!(Vote::decode(&[0, 0, 0, 3]), None);
        assert_eq!(Vote::decode(&[0, 0, 0, 3, 1, 0]), None);
    }

    #[test]
    fn tally_round_trips_and_rejects_bad_input() {
        let t = Tally {
            poll_id: 9,
            closed: true,
            close_hash: 42,
            counts: vec![3, 0, 65535],
        };
        assert_eq!(Tally::decode(&t.encode()), Some(t.clone()));
        let mut bad_flag = t.encode();
        bad_flag[4] = 2;
        assert_eq!(Tally::decode(&bad_flag), None);
        let one = Tally {
            counts: vec![1],
            ..t
        };
        assert_eq!(Tally::decode(&one.encode()), None);
    }

    #[test]
    fn voters_split_into_parts_that_fit() {
        let entries: Vec<VoterEntry> = (0..150)
            .map(|i| VoterEntry {
                option: (i % 10) as u8,
                name: format!("a rather long user name {i}"),
            })
            .collect();
        let parts = Voters::split(5, &entries);
        assert!(parts.len() > 1);
        let mut total = 0;
        for (i, part) in parts.iter().enumerate() {
            let bytes = part.encode();
            assert!(bytes.len() <= MAX_PAYLOAD);
            let decoded = Voters::decode(&bytes).expect("part decodes");
            assert_eq!(usize::from(decoded.part), i);
            assert_eq!(usize::from(decoded.parts), parts.len());
            assert!(
                decoded
                    .entries
                    .iter()
                    .all(|e| e.name.len() <= MAX_VOTER_NAME_BYTES)
            );
            total += decoded.entries.len();
        }
        assert_eq!(total, MAX_VOTERS);
    }

    #[test]
    fn empty_voter_list_is_one_part() {
        let parts = Voters::split(5, &[]);
        assert_eq!(parts.len(), 1);
        assert_eq!(Voters::decode(&parts[0].encode()), Some(parts[0].clone()));
    }

    #[test]
    fn voters_reject_bad_parts_and_options() {
        let mut v = Voters::split(
            1,
            &[VoterEntry {
                option: 0,
                name: "Anna".into(),
            }],
        )
        .remove(0);
        v.part = 1;
        assert_eq!(Voters::decode(&v.encode()), None, "part beyond parts");
        v.part = 0;
        v.entries[0].option = 10;
        assert_eq!(Voters::decode(&v.encode()), None, "option out of range");
    }

    #[test]
    fn truncate_keeps_characters_whole() {
        assert_eq!(truncate("äöü", 3), "ä");
        assert_eq!(truncate("abc", 10), "abc");
    }
}
