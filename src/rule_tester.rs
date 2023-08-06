use std::{any::TypeId, cmp::Ordering, iter, sync::Arc};

use better_any::Tid;
use derive_builder::Builder;
use tree_sitter_grep::{tree_sitter::Range, SupportedLanguage};

use crate::{
    config::{ConfigBuilder, ErrorLevel},
    context::FromFileRunContextInstanceProvider,
    rule::{Rule, RuleOptions},
    violation::{MessageOrMessageId, ViolationData, ViolationWithContext},
    FileRunContext, RuleConfiguration,
};

pub struct RuleTester {
    rule: Arc<dyn Rule>,
    rule_tests: RuleTests,
    language: SupportedLanguage,
}

impl RuleTester {
    fn new(rule: Arc<dyn Rule>, rule_tests: RuleTests) -> Self {
        if !rule.meta().fixable
            && rule_tests.invalid_tests.iter().any(|invalid_test| {
                matches!(
                    &invalid_test.output,
                    Some(RuleTestExpectedOutput::Output(_))
                )
            })
        {
            panic!("Specified 'output' for a non-fixable rule");
        }
        let languages = rule.meta().languages;
        if languages.len() != 1 {
            panic!("Only supporting single-language rules currently");
        }
        Self {
            language: languages[0],
            rule,
            rule_tests,
        }
    }

    pub fn run(rule: Arc<dyn Rule>, rule_tests: RuleTests) {
        Self::new(rule, rule_tests).run_tests()
    }

    fn run_tests(&self) {
        for valid_test in &self.rule_tests.valid_tests {
            self.run_valid_test(valid_test);
        }

        for invalid_test in &self.rule_tests.invalid_tests {
            self.run_invalid_test(invalid_test);
        }
    }

    fn run_valid_test(&self, valid_test: &RuleTestValid) {
        let violations = crate::run_for_slice(
            valid_test.code.as_bytes(),
            None,
            "tmp.rs",
            ConfigBuilder::default()
                .rule(&self.rule.meta().name)
                .all_standalone_rules([self.rule.clone()])
                .rule_configurations([RuleConfiguration {
                    name: self.rule.meta().name,
                    level: ErrorLevel::Error,
                    options: valid_test.options.clone(),
                }])
                .build()
                .unwrap(),
            self.language,
            get_dummy_from_file_run_context_instance_provider,
        );
        assert!(
            violations.is_empty(),
            "Valid case failed\ntest: {valid_test:#?}\nviolations: {violations:#?}"
        );
    }

    fn run_invalid_test(&self, invalid_test: &RuleTestInvalid) {
        let mut file_contents = invalid_test.code.clone().into_bytes();
        let violations = crate::run_fixing_for_slice(
            &mut file_contents,
            None,
            "tmp.rs",
            ConfigBuilder::default()
                .rule(&self.rule.meta().name)
                .all_standalone_rules([self.rule.clone()])
                .rule_configurations([RuleConfiguration {
                    name: self.rule.meta().name,
                    level: ErrorLevel::Error,
                    options: invalid_test.options.clone(),
                }])
                .fix(true)
                .report_fixed_violations(true)
                .build()
                .unwrap(),
            self.language,
            get_dummy_from_file_run_context_instance_provider,
        );
        assert_that_violations_match_expected(&violations, invalid_test);
        match invalid_test.output.as_ref() {
            Some(RuleTestExpectedOutput::Output(expected_file_contents)) => {
                assert_eq!(
                    std::str::from_utf8(&file_contents).unwrap(),
                    expected_file_contents,
                    "Didn't get expected output for code {:#?}, got: {violations:#?}",
                    invalid_test.code
                );
            }
            Some(RuleTestExpectedOutput::NoOutput) => {
                assert!(
                    !violations.iter().any(|violation| violation.had_fixes),
                    "Unexpected fixing violation was reported for code {:#?}, got: {violations:#?}",
                    invalid_test.code
                );
            }
            _ => (),
        }
    }
}

fn compare_ranges(a: Range, b: Range) -> Ordering {
    match a.start_byte.cmp(&b.start_byte) {
        Ordering::Equal => {}
        ord => return ord,
    }

    match a.end_byte.cmp(&b.end_byte) {
        Ordering::Equal => Ordering::Equal,
        Ordering::Less => Ordering::Greater,
        Ordering::Greater => Ordering::Less,
    }
}

fn assert_that_violations_match_expected(
    violations: &[ViolationWithContext],
    invalid_test: &RuleTestInvalid,
) {
    assert_eq!(
        violations.len(),
        invalid_test.errors.len(),
        "Didn't get expected number of violations for code {:#?}, got: {violations:#?}",
        invalid_test.code
    );
    let mut violations = violations.to_owned();
    // if https://github.com/tree-sitter/tree-sitter/pull/2460 gets merged then
    // could restore this to `violations.sort_by_key(|violation| violation.range)`
    violations.sort_by(|a, b| compare_ranges(a.range, b.range));
    for (violation, expected_violation) in iter::zip(violations, &invalid_test.errors) {
        assert_that_violation_matches_expected(&violation, expected_violation, invalid_test);
    }
}

fn assert_that_violation_matches_expected(
    violation: &ViolationWithContext,
    expected_violation: &RuleTestExpectedError,
    invalid_test: &RuleTestInvalid,
) {
    if let Some(message) = expected_violation.message.as_ref() {
        assert_eq!(
            message,
            &violation.message(),
            "Didn't get expected message for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
    if let Some(line) = expected_violation.line {
        assert_eq!(
            line,
            violation.range.start_point.row + 1,
            "Didn't get expected line for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
    if let Some(column) = expected_violation.column {
        assert_eq!(
            column,
            violation.range.start_point.column + 1,
            "Didn't get expected column for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
    if let Some(end_line) = expected_violation.end_line {
        assert_eq!(
            end_line,
            violation.range.end_point.row + 1,
            "Didn't get expected end line for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
    if let Some(end_column) = expected_violation.end_column {
        assert_eq!(
            end_column,
            violation.range.end_point.column + 1,
            "Didn't get expected end column for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
    if let Some(type_) = expected_violation.type_.as_ref() {
        assert_eq!(
            type_, violation.kind,
            "Didn't get expected type for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
    if let Some(message_id) = expected_violation.message_id.as_ref() {
        match &violation.message_or_message_id {
            MessageOrMessageId::MessageId(violation_message_id) => {
                assert_eq!(
                    violation_message_id, message_id,
                    "Didn't get expected message ID for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
            _ => panic!("Expected violation to use message ID"),
        }
    }
    if let Some(data) = expected_violation.data.as_ref() {
        assert_eq!(
            Some(data),
            violation.data.as_ref(),
            "Didn't get expected data for code {:#?}, got: {violation:#?}",
            invalid_test.code,
        );
    }
}

pub struct RuleTests {
    valid_tests: Vec<RuleTestValid>,
    invalid_tests: Vec<RuleTestInvalid>,
}

impl RuleTests {
    pub fn new(
        valid_tests: impl IntoIterator<Item = impl Into<RuleTestValid>>,
        invalid_tests: Vec<RuleTestInvalid>,
    ) -> Self {
        Self {
            valid_tests: valid_tests.into_iter().map(Into::into).collect(),
            invalid_tests,
        }
    }
}

#[derive(Debug)]
pub struct RuleTestValid {
    code: String,
    options: Option<RuleOptions>,
}

impl RuleTestValid {
    pub fn new(code: impl Into<String>, options: Option<RuleOptions>) -> Self {
        Self {
            code: code.into(),
            options,
        }
    }
}

impl From<&str> for RuleTestValid {
    fn from(value: &str) -> Self {
        Self::new(value, None)
    }
}

pub enum RuleTestExpectedOutput {
    Output(String),
    NoOutput,
}

impl From<String> for RuleTestExpectedOutput {
    fn from(value: String) -> Self {
        Self::Output(value)
    }
}

impl From<&str> for RuleTestExpectedOutput {
    fn from(value: &str) -> Self {
        Self::Output(value.to_owned())
    }
}

pub struct RuleTestInvalid {
    code: String,
    errors: Vec<RuleTestExpectedError>,
    output: Option<RuleTestExpectedOutput>,
    options: Option<RuleOptions>,
}

impl RuleTestInvalid {
    pub fn new(
        code: impl Into<String>,
        errors: impl IntoIterator<Item = impl Into<RuleTestExpectedError>>,
        output: Option<impl Into<RuleTestExpectedOutput>>,
        options: Option<RuleOptions>,
    ) -> Self {
        Self {
            code: code.into(),
            errors: errors.into_iter().map(Into::into).collect(),
            output: output.map(Into::into),
            options,
        }
    }
}

#[derive(Builder, Clone, Debug, Default)]
#[builder(default, setter(strip_option, into))]
pub struct RuleTestExpectedError {
    pub message: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub type_: Option<String>,
    pub message_id: Option<String>,
    pub data: Option<ViolationData>,
}

impl RuleTestExpectedError {
    pub fn with_type(&self, type_: &'static str) -> Self {
        Self {
            type_: Some(type_.to_owned()),
            ..self.clone()
        }
    }
}

impl From<&str> for RuleTestExpectedError {
    fn from(value: &str) -> Self {
        Self {
            message: Some(value.to_owned()),
            ..Default::default()
        }
    }
}

impl From<&RuleTestExpectedError> for RuleTestExpectedError {
    fn from(value: &RuleTestExpectedError) -> Self {
        value.clone()
    }
}

struct DummyFromFileRunContextInstanceProvider;

impl FromFileRunContextInstanceProvider for DummyFromFileRunContextInstanceProvider {
    fn get(&self, _id: TypeId, _file_run_context: FileRunContext<'_, '_>) -> Option<&dyn Tid> {
        unreachable!()
    }
}

pub fn get_dummy_from_file_run_context_instance_provider(
) -> Box<dyn FromFileRunContextInstanceProvider> {
    Box::new(DummyFromFileRunContextInstanceProvider)
}
