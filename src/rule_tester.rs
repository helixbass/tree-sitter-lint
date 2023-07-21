use std::{fs, process::Command};

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempdir::TempDir;

#[cfg(test)]
use crate::rule::Rule;

pub struct RuleTester {
    rule: Rule,
    rule_tests: RuleTests,
}

impl RuleTester {
    fn new(rule: Rule, rule_tests: RuleTests) -> Self {
        Self { rule, rule_tests }
    }

    pub fn run(rule: Rule, rule_tests: RuleTests) {
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
        let tmp_dir = TempDir::new("valid_test").unwrap();
        let test_filename = "tmp.rs";
        fs::write(tmp_dir.path().join(test_filename), &valid_test.code).unwrap();
        Command::cargo_bin("tree-sitter-lint")
            .unwrap()
            .args(["--rule", &self.rule.name])
            .current_dir(tmp_dir.path())
            .assert()
            .success()
            .stdout(predicate::str::is_empty());
    }

    fn run_invalid_test(&self, invalid_test: &RuleTestInvalid) {
        let tmp_dir = TempDir::new("invalid_test").unwrap();
        let test_filename = "tmp.rs";
        fs::write(tmp_dir.path().join(test_filename), &invalid_test.code).unwrap();
        Command::cargo_bin("tree-sitter-lint")
            .unwrap()
            .args(["--rule", &self.rule.name])
            .current_dir(tmp_dir.path())
            .assert()
            .failure()
            .code(1)
            .stdout(predicate::function(|stdout: &str| {
                does_invalid_output_match_expected(stdout, &invalid_test.errors)
            }));
    }
}

fn does_invalid_output_match_expected(
    stdout: &str,
    expected_errors: &[RuleTestExpectedError],
) -> bool {
    let mut lines: Vec<_> = stdout.split('\n').filter(|line| !line.is_empty()).collect();
    if lines.len() != expected_errors.len() {
        return false;
    }
    for expected_error in expected_errors {
        match lines
            .iter()
            .position(|&line| line.contains(&expected_error.message))
        {
            None => return false,
            Some(line_index) => {
                lines.remove(line_index);
            }
        }
    }
    true
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
}

impl RuleTestInvalid {
    pub fn new(
        code: impl Into<String>,
        errors: impl IntoIterator<Item = impl Into<RuleTestExpectedError>>,
    ) -> Self {
        Self {
            code: code.into(),
            errors: errors.into_iter().map(Into::into).collect(),
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
