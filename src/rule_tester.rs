use std::{iter, sync::Arc};

use crate::{config::ConfigBuilder, rule::Rule, violation::ViolationWithContext};

pub struct RuleTester {
    rule: Arc<dyn Rule>,
    rule_tests: RuleTests,
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
        Self { rule, rule_tests }
    }

    pub fn run(rule: impl Rule + 'static, rule_tests: RuleTests) {
        Self::new(Arc::new(rule), rule_tests).run_tests()
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
            "tmp.rs",
            ConfigBuilder::default()
                .rule(&self.rule.meta().name)
                .build()
                .unwrap(),
        );
        assert!(violations.is_empty());
    }

    fn run_invalid_test(&self, invalid_test: &RuleTestInvalid) {
        let mut file_contents = invalid_test.code.clone().into_bytes();
        let violations = crate::run_fixing_for_slice(
            &mut file_contents,
            "tmp.rs",
            ConfigBuilder::default()
                .rule(&self.rule.meta().name)
                .fix(true)
                .report_fixed_violations(true)
                .build()
                .unwrap(),
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
    assert_eq!(violations.len(), invalid_test.errors.len());
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
    assert_eq!(violation.message, expected_violation.message);
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

pub struct RuleTestValid {
    code: String,
}

impl RuleTestValid {
    pub fn new(code: impl Into<String>) -> Self {
        Self { code: code.into() }
    }
}

impl From<&str> for RuleTestValid {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

pub struct RuleTestInvalid {
    code: String,
    errors: Vec<RuleTestExpectedError>,
    output: Option<String>,
}

impl RuleTestInvalid {
    pub fn new(
        code: impl Into<String>,
        errors: impl IntoIterator<Item = impl Into<RuleTestExpectedError>>,
        output: Option<impl Into<String>>,
    ) -> Self {
        Self {
            code: code.into(),
            errors: errors.into_iter().map(Into::into).collect(),
            output: output.map(Into::into),
        }
    }
}

#[derive(Debug)]
pub struct RuleTestExpectedError {
    pub message: String,
}

impl RuleTestExpectedError {
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for RuleTestExpectedError {
    fn from(value: &str) -> Self {
        Self::new(value.to_owned())
    }
}
