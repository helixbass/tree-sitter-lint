use std::iter::Peekable;

use itertools::Itertools;
use squalid::{OptionExt, VecExt};
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
                    input_edit.old_end_byte <= prev_start_byte,
                    "Expected non-overlapping edits in reverse order"
                );
            }

            let (overlapping_edits, adjustment) = self.get_overlapping_edits(input_edit);
            match overlapping_edits {
                OverlappingEditsOrInsertionPoint::InsertionPoint(insertion_index) => {
                    self.edits.insert(
                        insertion_index,
                        AccumulatedEdit {
                            original_start_byte: (input_edit.start_byte as isize - adjustment)
                                .try_into()
                                .unwrap(),
                            original_len: input_edit.old_end_byte - input_edit.start_byte,
                            replacement_len: replacement.len(),
                            replacement_newline_offsets: get_newline_offsets(replacement).collect(),
                        },
                    );
                }
                OverlappingEditsOrInsertionPoint::OverlappingEdits(overlapping_indices) => {
                    let combined = overlapping_indices
                        .iter()
                        .copied()
                        .map(|index| self.edits[index].clone())
                        .reduce(|a, b| {
                            let gap =
                                b.original_start_byte - (a.original_start_byte + a.original_len);
                            AccumulatedEdit {
                                original_start_byte: a.original_start_byte,
                                original_len: a.original_len + b.original_len + gap,
                                replacement_len: a.replacement_len + b.replacement_len + gap,
                                replacement_newline_offsets: a
                                    .replacement_newline_offsets
                                    .and_extend(b.replacement_newline_offsets.into_iter().map(
                                        |newline_offset| newline_offset + a.replacement_len + gap,
                                    )),
                            }
                        })
                        .unwrap();
                    let input_edit_original_start = (input_edit.start_byte as isize - adjustment)
                        .try_into()
                        .unwrap();
                    let mut stretched = combined;
                    let left_stick_out =
                        stretched.original_start_byte as isize - input_edit_original_start as isize;
                    if left_stick_out > 0 {
                        let left_stick_out: usize = left_stick_out.try_into().unwrap();
                        stretched = AccumulatedEdit {
                            original_start_byte: input_edit_original_start,
                            original_len: left_stick_out + stretched.original_len,
                            replacement_len: left_stick_out + stretched.replacement_len,
                            replacement_newline_offsets: stretched
                                .replacement_newline_offsets
                                .into_iter()
                                .map(|newline_offset| left_stick_out + newline_offset)
                                .collect(),
                        };
                    }
                    let input_edit_old_len = input_edit.old_end_byte - input_edit.start_byte;
                    let input_edit_original_old_end =
                        input_edit_original_start + input_edit_old_len;
                    let right_stick_out = input_edit_original_old_end as isize
                        - (stretched.original_start_byte + stretched.replacement_len) as isize;
                    if right_stick_out > 0 {
                        let right_stick_out: usize = right_stick_out.try_into().unwrap();
                        stretched = AccumulatedEdit {
                            original_start_byte: stretched.original_start_byte,
                            original_len: stretched.original_len + right_stick_out,
                            replacement_len: stretched.replacement_len + right_stick_out,
                            replacement_newline_offsets: stretched.replacement_newline_offsets,
                        };
                    }
                    let input_edit_replacement_len =
                        input_edit.new_end_byte - input_edit.start_byte;
                    let input_edit_adjustment =
                        input_edit_replacement_len as isize - input_edit_old_len as isize;
                    let left_inset = input_edit_original_start - stretched.original_start_byte;
                    // let right_inset = (stretched.original_start_byte + stretched.replacement_len)
                    //     - input_edit_original_old_end;
                    let plopped = AccumulatedEdit {
                        original_start_byte: stretched.original_start_byte,
                        original_len: stretched.original_len,
                        replacement_len: (stretched.replacement_len as isize
                            + input_edit_adjustment)
                            .try_into()
                            .unwrap(),
                        replacement_newline_offsets: {
                            let mut replacement_newline_offsets: Vec<usize> = Default::default();
                            let mut index = 0;
                            while let Some(replacement_newline_offset) = stretched
                                .replacement_newline_offsets
                                .get(index)
                                .copied()
                                .filter(|&replacement_newline_offset| {
                                    replacement_newline_offset < left_inset
                                })
                            {
                                replacement_newline_offsets.push(replacement_newline_offset);
                                index += 1;
                            }
                            replacement_newline_offsets.extend(
                                get_newline_offsets(replacement)
                                    .map(|newline_offset| left_inset + newline_offset),
                            );
                            replacement_newline_offsets.extend(
                                stretched
                                    .replacement_newline_offsets
                                    .into_iter()
                                    .skip(index)
                                    .map(|replacement_newline_offset| {
                                        usize::try_from(
                                            (replacement_newline_offset as isize)
                                                + input_edit_adjustment,
                                        )
                                        .unwrap()
                                    }),
                            );
                            replacement_newline_offsets
                        },
                    };
                    self.edits.splice(
                        overlapping_indices.first().copied().unwrap()
                            ..=overlapping_indices.last().copied().unwrap(),
                        [plopped],
                    );
                }
            }

            prev_start_byte = Some(input_edit.start_byte);
        }
    }

    fn get_overlapping_edits(
        &self,
        input_edit: &InputEdit,
    ) -> (OverlappingEditsOrInsertionPoint, isize) {
        let mut adjustment = 0;
        let mut index = 0;
        let mut overlapping_indices: Vec<usize> = Default::default();
        let mut has_seen_overlap = false;
        while index < self.edits.len() {
            let existing_edit = &self.edits[index];
            let input_edit_original_start: usize = (input_edit.start_byte as isize - adjustment)
                .try_into()
                .unwrap();
            let input_edit_original_old_end =
                input_edit_original_start + (input_edit.old_end_byte - input_edit.start_byte);
            if input_edit_original_start
                >= existing_edit.original_start_byte + existing_edit.replacement_len
            {
                assert!(!has_seen_overlap);
                adjustment +=
                    existing_edit.replacement_len as isize - existing_edit.original_len as isize;
                index += 1;
                continue;
            }
            if input_edit_original_old_end <= existing_edit.original_start_byte {
                break;
            }
            has_seen_overlap = true;
            overlapping_indices.push(index);
            index += 1;
        }
        (
            match overlapping_indices.is_empty() {
                true => OverlappingEditsOrInsertionPoint::InsertionPoint(index),
                false => OverlappingEditsOrInsertionPoint::OverlappingEdits(overlapping_indices),
            },
            adjustment,
        )
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

#[derive(Clone)]
pub struct AccumulatedEdit {
    original_start_byte: usize,
    original_len: usize,
    replacement_len: usize,
    replacement_newline_offsets: Vec<usize>,
}

#[derive(Debug)]
enum OverlappingEditsOrInsertionPoint {
    OverlappingEdits(Vec<usize>),
    InsertionPoint(usize),
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
                get_input_edit_and_replacement(source_text, "foo::bar", "foo::b").0,
                get_input_edit_and_replacement(source_text, "baz::whee", "baz::hello").0,
            ]
        )
    }

    #[test]
    fn test_multiple_rounds_non_overlapping() {
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
        let updated_source_text = r#"use foo::b;
use bar::baz;
use baz::hello;
use whee::whoa;
"#;
        accumulated_edits.add_round_of_edits(&[
            get_input_edit_and_replacement(updated_source_text, "whee::whoa", "whee::whooo"),
            get_input_edit_and_replacement(updated_source_text, "bar::baz", "bar::bz"),
        ]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [
                get_input_edit_and_replacement(source_text, "foo::bar", "foo::b").0,
                get_input_edit_and_replacement(source_text, "bar::baz", "bar::bz").0,
                get_input_edit_and_replacement(source_text, "baz::whee", "baz::hello").0,
                get_input_edit_and_replacement(source_text, "whee::whoa", "whee::whooo").0,
            ]
        )
    }

    #[test]
    fn test_single_overlapping_sticks_out_both() {
        let source_text = r#"use foo::bar;
use bar::baz;
use baz::whee;
use whee::whoa;
"#;
        let original_newline_offsets = get_newline_offsets(source_text).collect_vec();
        assert_eq!(&original_newline_offsets, &[13, 27, 42, 58]);
        let mut accumulated_edits = AccumulatedEdits::new(original_newline_offsets);
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            source_text,
            "baz::whee",
            "baz::hello",
        )]);
        let updated_source_text = r#"use foo::bar;
use bar::baz;
use baz::hello;
use whee::whoa;
"#;
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            updated_source_text,
            "baz;\nuse baz::hello;",
            "{baz, foo};",
        )]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [get_input_edit_and_replacement(
                source_text,
                "bar::baz;\nuse baz::whee;",
                "bar::{baz, foo};"
            )
            .0,]
        )
    }

    #[test]
    fn test_single_overlapping_sticks_out_left() {
        let source_text = r#"use foo::bar;
use bar::baz;
use baz::whee;
use whee::whoa;
"#;
        let original_newline_offsets = get_newline_offsets(source_text).collect_vec();
        assert_eq!(&original_newline_offsets, &[13, 27, 42, 58]);
        let mut accumulated_edits = AccumulatedEdits::new(original_newline_offsets);
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            source_text,
            "baz::whee",
            "baz::hello",
        )]);
        let updated_source_text = r#"use foo::bar;
use bar::baz;
use baz::hello;
use whee::whoa;
"#;
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            updated_source_text,
            "baz;\nuse baz::h",
            "baaz;\nuse bzzz::w",
        )]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [get_input_edit_and_replacement(
                source_text,
                "bar::baz;\nuse baz::whee;",
                "bar::baaz;\nuse bazz::wello;"
            )
            .0,]
        )
    }

    #[test]
    fn test_single_overlapping_sticks_out_right() {
        let source_text = r#"use foo::bar;
use bar::baz;
use baz::whee;
use whee::whoa;
"#;
        let original_newline_offsets = get_newline_offsets(source_text).collect_vec();
        assert_eq!(&original_newline_offsets, &[13, 27, 42, 58]);
        let mut accumulated_edits = AccumulatedEdits::new(original_newline_offsets);
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            source_text,
            "baz::whee",
            "baz::hello",
        )]);
        let updated_source_text = r#"use foo::bar;
use bar::baz;
use baz::hello;
use whee::whoa;
"#;
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            updated_source_text,
            "hello;\nuse whee",
            "zhaa;\nuse whaa",
        )]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [get_input_edit_and_replacement(
                source_text,
                "use baz::whee;\nuse whee::whoa",
                "use baz::zhaa;\nuse whaa::whoa"
            )
            .0,]
        )
    }

    #[test]
    fn test_single_overlapping_sticks_out_neither() {
        let source_text = r#"use foo::bar;
use bar::baz;
use baz::whee;
use whee::whoa;
"#;
        let original_newline_offsets = get_newline_offsets(source_text).collect_vec();
        assert_eq!(&original_newline_offsets, &[13, 27, 42, 58]);
        let mut accumulated_edits = AccumulatedEdits::new(original_newline_offsets);
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            source_text,
            "baz::whee",
            "baz::hello",
        )]);
        let updated_source_text = r#"use foo::bar;
use bar::baz;
use baz::hello;
use whee::whoa;
"#;
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            updated_source_text,
            "hello",
            "hyzzzo",
        )]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [get_input_edit_and_replacement(source_text, "use baz::whee", "use baz::hyzzzo").0,]
        )
    }

    #[test]
    fn test_single_overlapping_combines_two_sticks_out_neither() {
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
            get_input_edit_and_replacement(source_text, "bar::baz", "bar::zzbze"),
        ]);
        let updated_source_text = r#"use foo::bar;
use bar::zzbze;
use baz::hello;
use whee::whoa;
"#;
        accumulated_edits.add_round_of_edits(&[get_input_edit_and_replacement(
            updated_source_text,
            "zzbze;\nuse baz::hello",
            "zzwzs;\nuse whaa::hyllo",
        )]);

        assert_eq!(
            accumulated_edits.get_input_edits(),
            [get_input_edit_and_replacement(
                source_text,
                "use bar::baz;\nuse baz::whee;",
                "use bar::zzwzs;\nuse whaa::hyllo;"
            )
            .0,]
        )
    }
}
