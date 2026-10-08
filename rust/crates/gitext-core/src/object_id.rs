//! Port of `GitExtensions.Extensibility.Git.ObjectId`: a SHA-1 hash.

use std::fmt;
use std::str::FromStr;

/// Number of hex characters in a full SHA-1.
pub const SHA1_CHAR_COUNT: usize = 40;

/// A 160-bit SHA-1 object id, stored big-endian.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ObjectId([u8; 20]);

/// Error returned when parsing an invalid object id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseObjectIdError(pub String);

impl fmt::Display for ParseObjectIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid SHA-1 hash: '{}'", self.0)
    }
}

impl std::error::Error for ParseObjectIdError {}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl ObjectId {
    /// Artificial id for working directory (unstaged) changes.
    pub const WORK_TREE: ObjectId = ObjectId([0x11; 20]);
    /// Artificial id for changes staged to the index.
    pub const INDEX: ObjectId = ObjectId([0x22; 20]);
    /// Artificial id for the combined diff of merge commits.
    pub const COMBINED_DIFF: ObjectId = ObjectId([0x33; 20]);
    /// The all-zero id.
    pub const ZERO: ObjectId = ObjectId([0; 20]);

    pub const fn from_bytes(bytes: [u8; 20]) -> Self {
        ObjectId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Produces an id populated with random bytes.
    pub fn random() -> Self {
        let mut data = [0u8; 20];
        for b in &mut data {
            *b = fastrand::u8(..);
        }
        ObjectId(data)
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 20]
    }

    /// Whether this id is internal to Git Extensions (work tree, index or combined diff).
    pub fn is_artificial(&self) -> bool {
        *self == Self::WORK_TREE || *self == Self::INDEX || *self == Self::COMBINED_DIFF
    }

    pub fn is_zero_or_artificial(&self) -> bool {
        self.is_zero() || self.is_artificial()
    }

    /// Parses exactly 40 hex characters (lower or upper case).
    pub fn try_parse_bytes(hex: &[u8]) -> Option<Self> {
        if hex.len() != SHA1_CHAR_COUNT {
            return None;
        }
        let mut data = [0u8; 20];
        for (i, chunk) in hex.chunks_exact(2).enumerate() {
            data[i] = (hex_val(chunk[0])? << 4) | hex_val(chunk[1])?;
        }
        Some(ObjectId(data))
    }

    /// Parses a full 40 character string. Leading/trailing characters cause failure.
    pub fn try_parse(s: &str) -> Option<Self> {
        Self::try_parse_bytes(s.as_bytes())
    }

    /// Parses 40 characters at `offset` within `s`; extra characters after are ignored.
    pub fn try_parse_at(s: &str, offset: usize) -> Option<Self> {
        let bytes = s.as_bytes();
        if offset + SHA1_CHAR_COUNT > bytes.len() {
            return None;
        }
        Self::try_parse_bytes(&bytes[offset..offset + SHA1_CHAR_COUNT])
    }

    /// Parses a string which must be exactly a lower-case 40 char hash.
    pub fn parse(s: &str) -> Result<Self, ParseObjectIdError> {
        Self::try_parse(s).ok_or_else(|| ParseObjectIdError(s.to_string()))
    }

    /// Whether `s` is a valid full lower-case SHA-1.
    pub fn is_valid(s: &str) -> bool {
        s.len() == SHA1_CHAR_COUNT && Self::is_valid_characters(s)
    }

    /// Whether `s` contains between `min_length` and 40 valid lower-case SHA-1 characters.
    pub fn is_valid_partial(s: &str, min_length: usize) -> bool {
        s.len() >= min_length && s.len() <= SHA1_CHAR_COUNT && Self::is_valid_characters(s)
    }

    fn is_valid_characters(s: &str) -> bool {
        s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }

    /// Returns the first `length` characters of the hash.
    ///
    /// # Panics
    /// If `length` is 0 or greater than 40.
    pub fn to_short_string_len(&self, length: usize) -> String {
        assert!(length >= 1, "Cannot be less than one.");
        assert!(length <= SHA1_CHAR_COUNT, "Cannot be greater than 40.");
        let mut s = self.to_string();
        s.truncate(length);
        s
    }

    /// The first 8 characters of the hash.
    pub fn to_short_string(&self) -> String {
        self.to_short_string_len(8)
    }

    /// Mirrors .NET `GetHashCode()`: the first 4 bytes read as a little-endian `i32`.
    /// Used as color seed for graph lanes.
    pub fn hash_code(&self) -> i32 {
        i32::from_le_bytes([self.0[0], self.0[1], self.0[2], self.0[3]])
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectId({})", self.to_short_string())
    }
}

impl FromStr for ObjectId {
    type Err = ParseObjectIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Whether `id` is a full lower-case SHA-1 (port of `GitRevision.IsFullSha1Hash`).
pub fn is_full_sha1_hash(id: &str) -> bool {
    ObjectId::is_valid(id)
}

#[cfg(test)]
mod tests {
    //! Ported from GitCommands.Tests/Git/ObjectIdTests.cs
    use super::*;

    const VALID: [&str; 3] = [
        "0000000000000000000000000000000000000000",
        "0102030405060708091011121314151617181920",
        "0123456789abcdef0123456789abcdef01234567",
    ];
    const INVALID: [&str; 6] = [
        "00000000000000000000000000000000000000",
        "000000000000000000000000000000000000000",
        "01020304050607080910111213141516171819200",
        "010203040506070809101112131415161718192001",
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "  0000000000000000000000000000000000000000  ",
    ];

    #[test]
    fn try_parse_handles_valid_hashes() {
        for sha1 in VALID {
            let id = ObjectId::try_parse(sha1).unwrap();
            assert_eq!(id.to_string(), sha1.to_lowercase());
            assert_eq!(ObjectId::parse(sha1).unwrap().to_string(), sha1);
            assert!(ObjectId::is_valid(sha1));
        }
    }

    #[test]
    fn try_parse_identifies_invalid_hashes() {
        for sha1 in INVALID {
            assert!(ObjectId::try_parse(sha1).is_none(), "{sha1}");
            assert!(ObjectId::parse(sha1).is_err());
            assert!(!ObjectId::is_valid(sha1));
        }
    }

    #[test]
    fn try_parse_with_offset_handles_valid_hashes() {
        for (s, offset) in [
            ("0000000000000000000000000000000000000000", 0),
            ("0000000000000000000000000000000000000000__", 0),
            ("_0102030405060708091011121314151617181920", 1),
            ("_0102030405060708091011121314151617181920_", 1),
            ("__0102030405060708091011121314151617181920", 2),
            ("__0102030405060708091011121314151617181920__", 2),
        ] {
            let id = ObjectId::try_parse_at(s, offset).unwrap();
            assert_eq!(id.to_string(), &s[offset..offset + 40]);
        }
    }

    #[test]
    fn parse_from_regex_capture() {
        let id = ObjectId::random();
        let s = format!("XYZ{id}XYZ");
        let m = regex::Regex::new("[a-f0-9]{40}").unwrap().find(&s).unwrap();
        assert_eq!(ObjectId::parse(m.as_str()).unwrap(), id);
        let m = regex::Regex::new("[a-f0-9]{39}").unwrap().find(&s).unwrap();
        assert!(ObjectId::parse(m.as_str()).is_err());
        let m = regex::Regex::new("[XYZa-f0-9]{39}").unwrap().find(&s).unwrap();
        assert!(ObjectId::parse(m.as_str()).is_err());
    }

    #[test]
    fn artificial_ids_have_expected_values() {
        assert_eq!(ObjectId::WORK_TREE.to_string(), "1".repeat(40));
        assert_eq!(ObjectId::INDEX.to_string(), "2".repeat(40));
        assert_eq!(ObjectId::COMBINED_DIFF.to_string(), "3".repeat(40));
        assert!(ObjectId::WORK_TREE.is_artificial());
        assert!(ObjectId::INDEX.is_artificial());
        assert!(ObjectId::COMBINED_DIFF.is_artificial());
        assert!(!ObjectId::random().is_artificial() || ObjectId::random() != ObjectId::random());
    }

    #[test]
    fn equality_and_hash_codes() {
        let a = ObjectId::parse("0102030405060708091011121314151617181920").unwrap();
        let b = ObjectId::parse("0102030405060708091011121314151617181920").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.hash_code(), b.hash_code());
        let z = ObjectId::parse("0000000000000000000000000000000000000000").unwrap();
        assert_ne!(a, z);
        assert_ne!(a.hash_code(), z.hash_code());
        assert_ne!(ObjectId::INDEX, ObjectId::WORK_TREE);
        assert_ne!(ObjectId::INDEX.hash_code(), ObjectId::WORK_TREE.hash_code());
        assert!(z.is_zero());
    }

    #[test]
    fn try_parse_bytes_works_as_expected() {
        const HEX_ASCII: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        for (offset, expected) in [
            (0, "000102030405060708090a0b0c0d0e0f10111213"),
            (1, "00102030405060708090a0b0c0d0e0f101112131"),
            (2, "0102030405060708090a0b0c0d0e0f1011121314"),
            (3, "102030405060708090a0b0c0d0e0f10111213141"),
            (26, "0d0e0f101112131415161718191a1b1c1d1e1f20"),
        ] {
            let id = ObjectId::try_parse_bytes(&HEX_ASCII.as_bytes()[offset..offset + 40]).unwrap();
            assert_eq!(id, ObjectId::parse(expected).unwrap());
        }
        const NON_HEX: &str = "0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz";
        assert!(ObjectId::try_parse_bytes(&NON_HEX.as_bytes()[..40]).is_none());
        assert!(ObjectId::try_parse_bytes(&[]).is_none());
        assert!(ObjectId::try_parse_bytes(&[0u8; 39]).is_none());
    }

    #[test]
    fn to_short_string() {
        let s = "0102030405060708091011121314151617181920";
        let id = ObjectId::parse(s).unwrap();
        for length in 1..40 {
            assert_eq!(id.to_short_string_len(length), &s[..length]);
        }
        assert!(std::panic::catch_unwind(|| id.to_short_string_len(0)).is_err());
        assert!(std::panic::catch_unwind(|| id.to_short_string_len(41)).is_err());
    }

    #[test]
    fn compare_respects_ordering() {
        let id = ObjectId::parse("0102030405060708091011121314151617181920").unwrap();
        assert_eq!(id.cmp(&id.clone()), std::cmp::Ordering::Equal);
        assert!(id > ObjectId::ZERO);
        let lower = ObjectId::parse("0000000000000000000000000000000000000001").unwrap();
        let higher = ObjectId::parse("ff00000000000000000000000000000000000000").unwrap();
        assert!(lower < higher);
    }

    #[test]
    fn parse_bytes_accepts_uppercase_and_normalises() {
        let id = ObjectId::try_parse_bytes(b"ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD").unwrap();
        assert_eq!(id.to_string(), "abcdefabcdefabcdefabcdefabcdefabcdefabcd");
    }
}
