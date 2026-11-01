//! Every number of the care loop in one place. The README's balance table mirrors this file.

/// Bond points needed for one Bond level.
pub const POINTS_PER_BOND: u32 = 100;
/// At most this many Bond points per local day, so growth takes days, not grinding.
pub const BOND_POINTS_PER_DAY: u32 = 100;

/// Bond needed for each stage after Hum.
pub const SPROUT_BOND: u32 = 3;
pub const BUDDY_BOND: u32 = 10;
pub const ECHO_BOND: u32 = 25;
/// The third colour of every species unlocks at this Bond (the Buddy stage).
pub const THIRD_COLOUR_BOND: u32 = BUDDY_BOND;

/// Meters run from 0 to this value.
pub const METER_MAX: u8 = 100;
pub const START_CHEER: u8 = 70;
pub const START_ENERGY: u8 = 80;
/// Cheer never falls below this on its own: the worst mood is "misses you".
pub const CHEER_FLOOR: u8 = 20;

/// Time in voice with others is counted in blocks of this many seconds …
pub const VOICE_BLOCK_SECS: u64 = 5 * 60;
/// … but only while the user talked within this many seconds (idling does not count).
pub const VOICE_TALK_WINDOW_SECS: u64 = 5 * 60;
pub const VOICE_BLOCK_NOTES: u32 = 1;
pub const VOICE_NOTES_PER_DAY: u32 = 24;
pub const VOICE_BLOCK_POINTS: u32 = 5;
pub const VOICE_BLOCK_CHEER: u8 = 3;
pub const VOICE_BLOCK_ENERGY_COST: u8 = 2;

/// Own chat message.
pub const CHAT_NOTES: u32 = 1;
pub const CHAT_NOTES_PER_DAY: u32 = 10;
pub const CHAT_POINTS: u32 = 1;
/// Someone else's chat message cheers the pet up a little.
pub const OTHERS_CHAT_CHEER: u8 = 1;

/// "Helped a friend": the user writes within this window after being mentioned.
pub const HELP_WINDOW_SECS: u64 = 2 * 60;
pub const HELP_NOTES: u32 = 3;
pub const HELPS_PER_DAY: u32 = 3;
pub const HELP_POINTS: u32 = 3;

/// "Greeted someone": the user talks within this window after someone joined the channel.
pub const GREET_WINDOW_SECS: u64 = 60;
pub const GREET_NOTES: u32 = 2;
pub const GREETS_PER_DAY: u32 = 5;
pub const GREET_POINTS: u32 = 2;

pub const PAT_CHEER: u8 = 5;
pub const PAT_POINTS: u32 = 2;
pub const PAT_POINTS_PER_DAY: u32 = 10;

pub const TREAT_COST: u32 = 5;
pub const TREAT_CHEER: u8 = 10;
pub const TREAT_ENERGY: u8 = 15;
pub const TREAT_POINTS: u32 = 5;

pub const PLAY_CHEER: u8 = 10;
pub const PLAY_ENERGY_COST: u8 = 10;
pub const PLAY_POINTS: u32 = 3;

pub const NAP_SECS: u64 = 30 * 60;
/// One Energy per this many seconds while napping (+60 over a whole nap).
pub const NAP_ENERGY_SECS: u64 = 30;
/// One Energy per this many seconds during Do Not Disturb or quiet time.
pub const DND_ENERGY_SECS: u64 = 3 * 60;
/// One Energy per this many seconds while the user is away (the pet naps).
pub const AWAY_ENERGY_SECS: u64 = 6 * 60;
/// One Cheer lost per this many seconds without company (2 per hour).
pub const CHEER_DECAY_SECS: u64 = 30 * 60;

/// Below this Energy the pet is sleepy; below this Cheer it misses you; from this Cheer up it is
/// cheerful.
pub const SLEEPY_ENERGY: u8 = 20;
pub const MISSES_YOU_CHEER: u8 = 35;
pub const CHEERFUL_CHEER: u8 = 70;

/// A gap between two ticks longer than this is treated as time away (e.g. the computer slept).
pub const MAX_TICK_GAP_SECS: u64 = 5 * 60;
/// Time away is counted up to this long; after that a reunion looks the same.
pub const MAX_AWAY_SECS: u64 = 30 * 24 * 60 * 60;

/// A streak survives one missed day per this many days.
pub const REST_DAY_EVERY: i64 = 7;

/// Pet names are at most this many characters.
pub const MAX_NAME_CHARS: usize = 16;
