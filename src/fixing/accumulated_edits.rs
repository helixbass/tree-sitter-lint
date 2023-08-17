use std::iter::Peekable;

use itertools::Itertools;
use squalid::OptionExt;
use tree_sitter_grep::tree_sitter::{InputEdit, Point};

pub struct AccumulatedEdits {
    original_newline_offsets: Vec<usize>,
    edits: Vec<AccumulatedEdit>,
}

impl AccumulatedEdits {
    pub fn new(original_newline_offsets: Vec<usize>) -> Self {
        Self {
            original_newline_offsets,
            edits: Default::default(),
        }
    }

    pub fn add_round_of_edits(&mut self, edits: &[(InputEdit, &str)]) {
        let mut prev_start_byte: Option<usize> = Default::default();
        for (input_edit, replacement) in edits {
            if let Some(prev_start_byte) = prev_start_byte {
                assert!(
                    input_edit.old_end_byte < prev_start_byte,
                    "Expected non-overlapping edits in reverse order"
                );
            }

            self.edits.push(AccumulatedEdit {
                original_start_byte: input_edit.start_byte,
                original_len: input_edit.old_end_byte - input_edit.start_byte,
                replacement_len: replacement.len(),
                replacement_newline_offsets: get_newline_offsets(replacement).collect(),
            });

            prev_start_byte = Some(input_edit.start_byte);
        }
    }

    pub fn get_input_edits(&self) -> Vec<InputEdit> {
        self.edits
            .iter()
            .map(|edit| {
                get_input_edit(
                    edit.original_start_byte,
                    edit.original_len,
                    edit.replacement_len,
                    &edit.replacement_newline_offsets,
                    &self.original_newline_offsets,
                )
            })
            .collect()
    }
}

pub struct AccumulatedEdit {
    original_start_byte: usize,
    original_len: usize,
    replacement_len: usize,
    replacement_newline_offsets: Vec<usize>,
}

fn get_point_from_newline_offsets(start_byte: usize, newline_offsets: &[usize]) -> Point {
    let row = newline_offsets
        .into_iter()
        .take_while(|&&newline_offset| newline_offset < start_byte)
        .count();
    Point {
        row,
        column: if row > 0 {
            start_byte - (newline_offsets[row - 1] + 1)
        } else {
            start_byte
        },
    }
}

fn get_newline_offsets(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.bytes()
        .enumerate()
        .filter_map(|(index, byte)| (byte == b'\n').then_some(index))
}

fn get_merged_newline_offsets(
    newline_offsets: &[usize],
    start_byte: usize,
    old_end_byte: usize,
    replacement_len: usize,
    replacement_newline_offsets: &[usize],
) -> Vec<usize> {
    let mut newline_offsets_iter = newline_offsets.into_iter().copied().peekable();
    let mut merged_newline_offsets = Vec::with_capacity(newline_offsets.len());
    let mut has_passed_replacement = false;
    let adjustment = (replacement_len as isize) - (old_end_byte - start_byte) as isize;

    fn push_all_replacement_newline_offsets(
        merged_newline_offsets: &mut Vec<usize>,
        replacement_newline_offsets: &[usize],
        start_byte: usize,
    ) {
        for &replacement_newline_offset in replacement_newline_offsets {
            merged_newline_offsets.push(start_byte + replacement_newline_offset);
        }
    }

    fn push_adjusted_existing_newline_offset(
        merged_newline_offsets: &mut Vec<usize>,
        newline_offset: usize,
        adjustment: isize,
    ) {
        merged_newline_offsets.push((newline_offset as isize + adjustment).try_into().unwrap());
    }

    fn skip_all_replaced_newline_offsets(
        newline_offsets_iter: &mut Peekable<impl Iterator<Item = usize>>,
        old_end_byte: usize,
    ) {
        while newline_offsets_iter
            .peek()
            .matches(|&newline_offset| newline_offset < old_end_byte)
        {
            newline_offsets_iter.next().unwrap();
        }
    }

    loop {
        match newline_offsets_iter.next() {
            Some(newline_offset) => match has_passed_replacement {
                false => match newline_offset >= start_byte {
                    false => merged_newline_offsets.push(newline_offset),
                    true => {
                        has_passed_replacement = true;
                        push_all_replacement_newline_offsets(
                            &mut merged_newline_offsets,
                            &replacement_newline_offsets,
                            start_byte,
                        );
                        match newline_offset >= old_end_byte {
                            false => {
                                skip_all_replaced_newline_offsets(
                                    &mut newline_offsets_iter,
                                    old_end_byte,
                                );
                            }
                            true => {
                                push_adjusted_existing_newline_offset(
                                    &mut merged_newline_offsets,
                                    newline_offset,
                                    adjustment,
                                );
                            }
                        }
                    }
                },
                true => push_adjusted_existing_newline_offset(
                    &mut merged_newline_offsets,
                    newline_offset,
                    adjustment,
                ),
            },
            None => match has_passed_replacement {
                false => {
                    push_all_replacement_newline_offsets(
                        &mut merged_newline_offsets,
                        &replacement_newline_offsets,
                        start_byte,
                    );
                    return merged_newline_offsets;
                }
                true => return merged_newline_offsets,
            },
        }
    }
}

fn get_merged_newline_offsets_from_replacement(
    newline_offsets: &[usize],
    start_byte: usize,
    old_end_byte: usize,
    replacement: &str,
) -> Vec<usize> {
    get_merged_newline_offsets(
        newline_offsets,
        start_byte,
        old_end_byte,
        replacement.len(),
        &get_newline_offsets(replacement).collect_vec(),
    )
}

fn get_input_edit(
    start_byte: usize,
    original_len: usize,
    replacement_len: usize,
    replacement_newline_offsets: &[usize],
    original_newline_offsets: &[usize],
) -> InputEdit {
    let old_end_byte = start_byte + original_len;
    let new_end_byte = start_byte + replacement_len;
    let updated_newline_offsets = get_merged_newline_offsets(
        original_newline_offsets,
        start_byte,
        old_end_byte,
        replacement_len,
        replacement_newline_offsets,
    );

    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position: get_point_from_newline_offsets(start_byte, original_newline_offsets),
        old_end_position: get_point_from_newline_offsets(old_end_byte, original_newline_offsets),
        new_end_position: get_point_from_newline_offsets(new_end_byte, &updated_newline_offsets),
    }
}

fn get_input_edit_from_replacement(
    start_byte: usize,
    original_len: usize,
    replacement: &str,
    original_newline_offsets: &[usize],
) -> InputEdit {
    get_input_edit(
        start_byte,
        original_len,
        replacement.len(),
        &get_newline_offsets(replacement).collect_vec(),
        original_newline_offsets,
    )
}

#[cfg(test)]
mod tests {
    use std::iter;

    use itertools::Itertools;

    use super::*;

    #[test]
    fn test_get_newline_offsets() {
        assert_eq!(
            get_newline_offsets(
                r#"use foo::bar();
fn whee() {
    whoa();
}"#,
            )
            .collect_vec(),
            [15, 27, 39]
        )
    }

    #[test]
    fn test_get_newline_offsets_leading_and_trailing() {
        assert_eq!(
            get_newline_offsets(
                r#"
use foo::bar();
fn whee() {
    whoa();
}
"#,
            )
            .collect_vec(),
            [0, 16, 28, 40, 42]
        )
    }

    #[test]
    fn test_get_merged_newline_offsets_simple_replacement() {
        assert_eq!(
            get_merged_newline_offsets_from_replacement(
                &get_newline_offsets(
                    r#"use foo::bar();
fn whee() {
    whoa();
}"#
                )
                .collect_vec(),
                19,
                23,
                "wheee"
            ),
            [15, 28, 40]
        )
    }

    #[test]
    fn test_get_merged_newline_offsets_replacement_after_all_newlines() {
        assert_eq!(
            get_merged_newline_offsets_from_replacement(
                &get_newline_offsets(
                    r#"use foo::bar();
fn whee() {
    whoa();
}"#
                )
                .collect_vec(),
                41,
                41,
                " // great"
            ),
            [15, 27, 39]
        )
    }

    #[test]
    fn test_get_merged_newline_offsets_replacement_before_all_newlines() {
        assert_eq!(
            get_merged_newline_offsets_from_replacement(
                &get_newline_offsets(
                    r#"use foo::bar();
fn whee() {
    whoa();
}"#
                )
                .collect_vec(),
                4,
                7,
                "fooo"
            ),
            [16, 28, 40]
        )
    }

    #[test]
    fn test_get_merged_newline_offsets_replacement_contains_newlines() {
        assert_eq!(
            get_merged_newline_offsets_from_replacement(
                &get_newline_offsets(
                    r#"use foo::bar();
fn whee() {
    whoa();
}"#
                )
                .collect_vec(),
                32,
                32,
                "whooo();\n    "
            ),
            [15, 27, 40, 52]
        )
    }

    #[test]
    fn test_get_merged_newline_offsets_replacement_replaces_newlines() {
        assert_eq!(
            get_merged_newline_offsets_from_replacement(
                &get_newline_offsets(
                    r#"use foo::bar();
fn whee() {
    whoa();
}"#
                )
                .collect_vec(),
                27,
                40,
                " whoa() "
            ),
            [15]
        )
    }

    fn get_input_edit_and_replacement<'a>(
        text: &str,
        original_chunk: &str,
        replacement_chunk: &'a str,
    ) -> (InputEdit, &'a str) {
        let chunk_start_byte = text.find(original_chunk).unwrap();
        assert!(
            !text[chunk_start_byte + 1..].contains(original_chunk),
            "Non-unique chunk"
        );
        let shared_chunk_prefix = iter::zip(original_chunk.chars(), replacement_chunk.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a)
            .collect::<String>();
        let shared_chunk_suffix = iter::zip(
            original_chunk.chars().rev(),
            replacement_chunk.chars().rev(),
        )
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a)
        .collect_vec()
        .into_iter()
        .rev()
        .collect::<String>();
        let replacement = &replacement_chunk
            [shared_chunk_prefix.len()..replacement_chunk.len() - shared_chunk_suffix.len()];
        let replacement_offset_in_chunk = shared_chunk_prefix.len();
        let replaced = &original_chunk
            [shared_chunk_prefix.len()..original_chunk.len() - shared_chunk_suffix.len()];

        (
            get_input_edit_from_replacement(
                chunk_start_byte + replacement_offset_in_chunk,
                replaced.len(),
                replacement,
                &get_newline_offsets(text).collect_vec(),
            ),
            replacement,
        )
    }

    #[test]
    fn test_get_input_edit_and_replacement() {
        assert_eq!(
            get_input_edit_and_replacement(
                r#"use foo::bar;
use bar::baz;
use baz::whee;
use whee::whoa;
"#,
                "baz::whee",
                "baz::hello"
            ),
            (
                InputEdit {
                    start_byte: 37,
                    old_end_byte: 41,
                    new_end_byte: 42,
                    start_position: Point { row: 2, column: 9 },
                    old_end_position: Point { row: 2, column: 13 },
                    new_end_position: Point { row: 2, column: 14 },
                },
                "hello"
            )
        );
    }

    #[test]
    fn test_single_round_of_edits() {
        let source_text = r#"use foo::bar;
use bar::baz;
use baz::whee;
use whee::whoa;
"#;
        let original_newline_offsets = get_newline_offsets(source_text).collect_vec();
        assert_eq!(&original_newline_offsets, &[13, 27, 42, 58]);
        let mut accumulated_edits = AccumulatedEdits::new(original_newline_offsets);
        accumulated_edits.add_round_of_edits(&[
            get_input_edit_and_replacement(source_text, "baz::whee", "baz::hello"),
            get_input_edit_and_replacement(source_text, "foo::bar", "foo::b"),
        ]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [
                get_input_edit_and_replacement(source_text, "baz::whee", "baz::hello").0,
                get_input_edit_and_replacement(source_text, "foo::bar", "foo::b").0,
            ]
        )
    }
}
