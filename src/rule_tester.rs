use std::{collections::HashMap, iter, sync::Arc};

use derive_builder::Builder;
use tree_sitter_grep::SupportedLanguage;

use crate::{
    config::{ConfigBuilder, ErrorLevel},
    rule::{Rule, RuleOptions},
    violation::{MessageOrMessageId, ViolationWithContext},
    RuleConfiguration,
};

pub struct RuleTester {
    rule: Arc<dyn Rule>,
    rule_tests: RuleTests,
    language: SupportedLanguage,
}

impl RuleTester {
    fn new(rule: Arc<dyn Rule>, rule_tests: RuleTests) -> Self {
        if !rule.meta().fixable
            && rule_tests
                .invalid_tests
                .iter()
                .any(|invalid_test| invalid_test.output.is_some())
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
        );
        if let Some(expected_file_contents) = invalid_test.output.as_ref() {
            assert_eq!(&file_contents, expected_file_contents.as_bytes());
        }
        assert_that_violations_match_expected(&violations, invalid_test);
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
    violations.sort_by_key(|violation| violation.range);
    for (violation, expected_violation) in iter::zip(violations, &invalid_test.errors) {
        assert_that_violation_matches_expected(&violation, expected_violation);
    }
}

fn assert_that_violation_matches_expected(
    violation: &ViolationWithContext,
    expected_violation: &RuleTestExpectedError,
) {
    if let Some(message) = expected_violation.message.as_ref() {
        assert_eq!(message, &violation.message());
    }
    if let Some(line) = expected_violation.line {
        assert_eq!(line, violation.range.start_point.row + 1);
    }
    if let Some(column) = expected_violation.column {
        assert_eq!(column, violation.range.start_point.column + 1);
    }
    if let Some(end_line) = expected_violation.end_line {
        assert_eq!(end_line, violation.range.end_point.row + 1);
    }
    if let Some(end_column) = expected_violation.end_column {
        assert_eq!(end_column, violation.range.end_point.column + 1);
    }
    if let Some(type_) = expected_violation.type_.as_ref() {
        assert_eq!(type_, violation.kind);
    }
    if let Some(message_id) = expected_violation.message_id.as_ref() {
        match &violation.message_or_message_id {
            MessageOrMessageId::MessageId(violation_message_id) => {
                assert_eq!(violation_message_id, message_id);
            }
            _ => panic!("Expected violation to use message ID"),
        }
    }
    if let Some(data) = expected_violation.data.as_ref() {
        assert_eq!(Some(data), violation.data.as_ref());
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

pub struct RuleTestInvalid {
    code: String,
    errors: Vec<RuleTestExpectedError>,
    output: Option<String>,
    options: Option<RuleOptions>,
}

impl RuleTestInvalid {
    pub fn new(
        code: impl Into<String>,
        errors: impl IntoIterator<Item = impl Into<RuleTestExpectedError>>,
        output: Option<impl Into<String>>,
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
    pub data: Option<HashMap<String, String>>,
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
