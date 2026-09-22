pub struct ContentRange {
    pub utf8_char_start: usize,
    pub utf8_char_end: usize,
    pub bytes_start: usize,
    pub bytes_end: usize,
}

pub enum TransformOperation {
    Replace {
        range: ContentRange,
        replacement: String,
    },
    Delete {
        range: ContentRange,
    },

    Insert {
        range: ContentRange,
        insertion: String,
    },
}

pub enum ParserKind {
    /// Filter parser that applies transformations based on specific rules.
    Filter,

    /// Table of contents parser that generates a structured representation of headings.
    Toc,
}
