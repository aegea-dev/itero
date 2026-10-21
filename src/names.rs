//! Random names: an adjective and a noun, `quiet-river`. A label to say
//! aloud and type; the UUID stays the identity.
use std::hash::{BuildHasher, Hasher, RandomState};

const ADJECTIVES: [&str; 64] = [
    "amber", "bold", "brave", "bright", "brisk", "calm", "clever", "cool", "cozy", "crisp",
    "curly", "dawn", "deep", "eager", "early", "fair", "fancy", "fast", "fresh", "gentle", "glad",
    "golden", "grand", "green", "happy", "hidden", "honest", "humble", "jolly", "keen", "kind",
    "late", "lively", "lucky", "merry", "misty", "modest", "neat", "noble", "odd", "pale",
    "patient", "plain", "polite", "proud", "quick", "quiet", "rapid", "rare", "ready", "rosy",
    "shy", "silent", "silver", "simple", "sleepy", "smooth", "snowy", "steady", "sunny", "swift",
    "tidy", "warm", "wild",
];

const NOUNS: [&str; 64] = [
    "acorn", "anchor", "apple", "badger", "bay", "beacon", "birch", "brook", "canyon", "cedar",
    "cliff", "cloud", "comet", "coral", "creek", "delta", "dune", "eagle", "ember", "falcon",
    "fern", "field", "finch", "fjord", "forest", "fox", "garden", "glacier", "grove", "harbor",
    "hawk", "heron", "hill", "island", "lake", "lantern", "maple", "meadow", "moon", "moss", "oak",
    "ocean", "otter", "owl", "pebble", "pine", "planet", "pond", "rain", "raven", "reef", "river",
    "robin", "sparrow", "spring", "star", "stone", "stream", "summit", "thunder", "tide", "valley",
    "willow", "wolf",
];

/// A new random name.
pub fn random() -> String {
    let n = RandomState::new().build_hasher().finish();
    let a = ADJECTIVES[(n % 64) as usize];
    let b = NOUNS[((n >> 6) % 64) as usize];
    format!("{a}-{b}")
}
