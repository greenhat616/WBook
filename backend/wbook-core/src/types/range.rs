use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RangeError {
    #[error("invalid range: start `{start}` is greater than end `{end}`")]
    Inverted { start: u64, end: u64 },
}

macro_rules! range_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
        pub struct $name {
            pub start: u64,
            pub end: u64,
        }

        impl $name {
            pub fn new(start: u64, end: u64) -> Result<Self, RangeError> {
                if start > end {
                    return Err(RangeError::Inverted { start, end });
                }
                Ok(Self { start, end })
            }

            pub fn len(&self) -> u64 {
                self.end - self.start
            }

            pub fn is_empty(&self) -> bool {
                self.start == self.end
            }

            pub fn contains(&self, other: &Self) -> bool {
                self.start <= other.start && other.end <= self.end
            }

            /// Merge two ranges into the smallest range covering both.
            /// Used to backfill a container node's range from its children.
            pub fn merge(self, other: Self) -> Self {
                Self {
                    start: self.start.min(other.start),
                    end: self.end.max(other.end),
                }
            }
        }
    };
}

range_type!(
    TextRange,
    "A range of byte offsets into the decoded (UTF-8) text."
);
range_type!(
    ByteRange,
    "A range of byte offsets into the raw file content."
);
