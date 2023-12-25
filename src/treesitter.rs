use tree_sitter_grep::tree_sitter::Range;

pub fn range_between_starts(a: Range, b: Range) -> Range {
    Range {
        start_byte: a.start_byte,
        end_byte: b.start_byte,
        start_point: a.start_point,
        end_point: b.start_point,
    }
}

pub fn range_between_start_and_end(a: Range, b: Range) -> Range {
    Range {
        start_byte: a.start_byte,
        end_byte: b.end_byte,
        start_point: a.start_point,
        end_point: b.end_point,
    }
}

pub fn range_between_end_and_start(a: Range, b: Range) -> Range {
    Range {
        start_byte: a.end_byte,
        end_byte: b.start_byte,
        start_point: a.end_point,
        end_point: b.start_point,
    }
}

pub fn range_between_ends(a: Range, b: Range) -> Range {
    Range {
        start_byte: a.end_byte,
        end_byte: b.end_byte,
        start_point: a.end_point,
        end_point: b.end_point,
    }
}
