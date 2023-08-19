use std::{any::TypeId, cell::RefCell, cmp::Ordering, env, iter, marker::PhantomData, sync::Arc};

use better_any::Tid;
use derive_builder::Builder;
use squalid::NonEmpty;
use tree_sitter_grep::{tree_sitter::Range, SupportedLanguage};

use crate::{
    config::{ConfigBuilder, ErrorLevel},
    context::FromFileRunContextInstanceProvider,
    rule::{Rule, RuleOptions},
    violation::{MessageOrMessageId, ViolationData, ViolationWithContext},
    FileRunContext, FixingForSliceRunStatus, FromFileRunContextInstanceProviderFactory, Plugin,
    RuleConfiguration,
};

pub struct RuleTester {
    rule: Arc<dyn Rule>,
    rule_tests: RuleTests,
    language: SupportedLanguage,
    from_file_run_context_instance_provider_factory:
        Box<dyn FromFileRunContextInstanceProviderFactory>,
    plugins: Vec<Plugin>,
    should_aggregate_results: bool,
    aggregated_results: RefCell<Vec<TestResult>>,
}

impl RuleTester {
    fn new(
        rule: Arc<dyn Rule>,
        rule_tests: RuleTests,
        from_file_run_context_instance_provider_factory: Box<
            dyn FromFileRunContextInstanceProviderFactory,
        >,
        plugins: Vec<Plugin>,
    ) -> Self {
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
        let languages = &rule.meta().languages;
        if languages.len() != 1 {
            panic!("Only supporting single-language rules currently");
        }
        Self {
            language: languages[0],
            rule,
            rule_tests,
            from_file_run_context_instance_provider_factory,
            plugins,
            should_aggregate_results: env::var("RULE_TEST_SUMMARY").ok().is_non_empty(),
            aggregated_results: Default::default(),
        }
    }

    pub fn run(rule: Arc<dyn Rule>, rule_tests: RuleTests) {
        Self::new(
            rule,
            rule_tests,
            Box::new(DummyFromFileRunContextInstanceProviderFactory),
            Default::default(),
        )
        .run_tests()
    }

    pub fn run_with_from_file_run_context_instance_provider(
        rule: Arc<dyn Rule>,
        rule_tests: RuleTests,
        from_file_run_context_instance_provider_factory: Box<
            dyn FromFileRunContextInstanceProviderFactory,
        >,
    ) {
        Self::new(
            rule,
            rule_tests,
            from_file_run_context_instance_provider_factory,
            Default::default(),
        )
        .run_tests()
    }

    pub fn run_with_plugins(rule: Arc<dyn Rule>, rule_tests: RuleTests, plugins: Vec<Plugin>) {
        Self::new(
            rule,
            rule_tests,
            Box::new(DummyFromFileRunContextInstanceProviderFactory),
            plugins,
        )
        .run_tests()
    }

    fn run_tests(&self) {
        if let Some(only_valid_test) = self
            .rule_tests
            .valid_tests
            .iter()
            .find(|valid_test| valid_test.only == Some(true))
        {
            self.run_valid_test(only_valid_test);
            return;
        }

        if let Some(only_invalid_test) = self
            .rule_tests
            .invalid_tests
            .iter()
            .find(|invalid_test| invalid_test.only == Some(true))
        {
            self.run_invalid_test(only_invalid_test);
            return;
        }

        for valid_test in &self.rule_tests.valid_tests {
            self.run_valid_test(valid_test);
        }

        for invalid_test in &self.rule_tests.invalid_tests {
            self.run_invalid_test(invalid_test);
        }

        if self.should_aggregate_results {
            let mut saw_failure = false;
            self.aggregated_results
                .borrow()
                .iter()
                .for_each(|test_result| {
                    use colored::Colorize;

                    match test_result.outcome {
                        TestOutcome::Passed => {
                            println!(
                                "{} {} {}",
                                test_result
                                    .code
                                    .trim()
                                    .chars()
                                    .take(30)
                                    .collect::<String>()
                                    .dimmed(),
                                if test_result.was_invalid { "i" } else { "v" },
                                "✔".green()
                            );
                        }
                        TestOutcome::Failed => {
                            saw_failure = true;
                            println!(
                                "{} {} {}",
                                test_result
                                    .code
                                    .trim()
                                    .chars()
                                    .take(30)
                                    .collect::<String>()
                                    .dimmed(),
                                if test_result.was_invalid { "i" } else { "v" },
                                "✗".red()
                            );
                        }
                    }
                });

            if saw_failure {
                panic!("Rule test failed");
            }
        }
    }

    fn run_valid_test(&self, valid_test: &RuleTestValid) {
        let (violations, _) = crate::run_for_slice(
            valid_test.code.as_bytes(),
            None,
            "tmp.rs",
            ConfigBuilder::default()
                .rule(self.rule.meta().name.clone())
                .all_standalone_rules([self.rule.clone()])
                .rule_configurations([RuleConfiguration {
                    name: self.rule.meta().name.clone(),
                    level: ErrorLevel::Error,
                    options: valid_test.options.clone(),
                }])
                .all_plugins(self.plugins.clone())
                .build()
                .unwrap(),
            self.language,
            &*self.from_file_run_context_instance_provider_factory,
        );

        if self.should_aggregate_results {
            self.aggregated_results
                .borrow_mut()
                .push(if !violations.is_empty() {
                    TestResult {
                        outcome: TestOutcome::Failed,
                        code: valid_test.code.clone(),
                        was_invalid: false,
                    }
                } else {
                    TestResult {
                        outcome: TestOutcome::Passed,
                        code: valid_test.code.clone(),
                        was_invalid: false,
                    }
                });
            return;
        }

        assert!(
            violations.is_empty(),
            "Valid case failed\ntest: {valid_test:#?}\nviolations: {violations:#?}"
        );
    }

    fn run_invalid_test(&self, invalid_test: &RuleTestInvalid) {
        let mut file_contents = invalid_test.code.clone().into_bytes();
        let FixingForSliceRunStatus { violations, .. } = crate::run_fixing_for_slice(
            &mut file_contents,
            None,
            "tmp.rs",
            ConfigBuilder::default()
                .rule(self.rule.meta().name.clone())
                .all_standalone_rules([self.rule.clone()])
                .rule_configurations([RuleConfiguration {
                    name: self.rule.meta().name.clone(),
                    level: ErrorLevel::Error,
                    options: invalid_test.options.clone(),
                }])
                .all_plugins(self.plugins.clone())
                .fix(true)
                .report_fixed_violations(true)
                .single_fixing_pass(true)
                .build()
                .unwrap(),
            self.language,
            &*self.from_file_run_context_instance_provider_factory,
            Default::default(),
        );

        if !self.check_that_violations_match_expected(&violations, invalid_test) {
            return;
        }

        match invalid_test.output.as_ref() {
            Some(RuleTestExpectedOutput::Output(expected_file_contents)) => {
                if self.should_aggregate_results {
                    if std::str::from_utf8(&file_contents).unwrap() != expected_file_contents {
                        self.aggregated_results.borrow_mut().push(TestResult {
                            outcome: TestOutcome::Failed,
                            code: invalid_test.code.clone(),
                            was_invalid: true,
                        });
                        return;
                    }
                } else {
                    assert_eq!(
                        std::str::from_utf8(&file_contents).unwrap(),
                        expected_file_contents,
                        "Didn't get expected output for code {:#?}, got: {violations:#?}",
                        invalid_test.code
                    );
                }
            }
            Some(RuleTestExpectedOutput::NoOutput) => {
                if self.should_aggregate_results {
                    if violations.iter().any(|violation| violation.had_fixes) {
                        self.aggregated_results.borrow_mut().push(TestResult {
                            outcome: TestOutcome::Failed,
                            code: invalid_test.code.clone(),
                            was_invalid: true,
                        });
                        return;
                    }
                } else {
                    assert!(
                    !violations.iter().any(|violation| violation.had_fixes),
                    "Unexpected fixing violation was reported for code {:#?}, got: {violations:#?}",
                    invalid_test.code
                );
                }
            }
            _ => (),
        }

        self.aggregated_results.borrow_mut().push(TestResult {
            outcome: TestOutcome::Passed,
            code: invalid_test.code.clone(),
            was_invalid: true,
        });
    }

    fn check_that_violations_match_expected(
        &self,
        violations: &[ViolationWithContext],
        invalid_test: &RuleTestInvalid,
    ) -> bool {
        if self.should_aggregate_results {
            if violations.len()
                != match &invalid_test.errors {
                    RuleTestExpectedErrors::NumErrors(num_errors) => *num_errors,
                    RuleTestExpectedErrors::Errors(errors) => errors.len(),
                }
            {
                self.aggregated_results.borrow_mut().push(TestResult {
                    outcome: TestOutcome::Failed,
                    code: invalid_test.code.clone(),
                    was_invalid: true,
                });
                return false;
            }
        } else {
            assert_eq!(
                violations.len(),
                match &invalid_test.errors {
                    RuleTestExpectedErrors::NumErrors(num_errors) => *num_errors,
                    RuleTestExpectedErrors::Errors(errors) => errors.len(),
                },
                "Didn't get expected number of violations for code {:#?}, got: {violations:#?}",
                invalid_test.code
            );
        }

        if let RuleTestExpectedErrors::Errors(errors) = &invalid_test.errors {
            let mut violations = violations.to_owned();
            violations.sort_by(|a, b| compare_ranges(a.range, b.range));
            for (violation, expected_violation) in iter::zip(violations, errors) {
                if !self.check_that_violation_matches_expected(
                    &violation,
                    expected_violation,
                    invalid_test,
                ) {
                    return false;
                }
            }
        }

        true
    }

    fn check_that_violation_matches_expected(
        &self,
        violation: &ViolationWithContext,
        expected_violation: &RuleTestExpectedError,
        invalid_test: &RuleTestInvalid,
    ) -> bool {
        if let Some(message) = expected_violation.message.as_ref() {
            if self.should_aggregate_results {
                if message != &violation.message() {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    message,
                    &violation.message(),
                    "Didn't get expected message for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        if let Some(line) = expected_violation.line {
            if self.should_aggregate_results {
                if line != violation.range.start_point.row + 1 {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    line,
                    violation.range.start_point.row + 1,
                    "Didn't get expected line for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        if let Some(column) = expected_violation.column {
            if self.should_aggregate_results {
                if column != violation.range.start_point.column + 1 {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    column,
                    violation.range.start_point.column + 1,
                    "Didn't get expected column for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        if let Some(end_line) = expected_violation.end_line {
            if self.should_aggregate_results {
                if end_line != violation.range.end_point.row + 1 {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    end_line,
                    violation.range.end_point.row + 1,
                    "Didn't get expected end line for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        if let Some(end_column) = expected_violation.end_column {
            if self.should_aggregate_results {
                if end_column != violation.range.end_point.column + 1 {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    end_column,
                    violation.range.end_point.column + 1,
                    "Didn't get expected end column for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        if let Some(type_) = expected_violation.type_.as_ref() {
            if self.should_aggregate_results {
                if type_ != violation.kind {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    type_, violation.kind,
                    "Didn't get expected type for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        if let Some(message_id) = expected_violation.message_id.as_ref() {
            if self.should_aggregate_results {
                if !matches!(
                    &violation.message_or_message_id,
                    MessageOrMessageId::MessageId(violation_message_id) if violation_message_id == message_id,
                ) {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
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
        }

        if let Some(data) = expected_violation.data.as_ref() {
            if self.should_aggregate_results {
                if Some(data) != violation.data.as_ref() {
                    self.aggregated_results.borrow_mut().push(TestResult {
                        outcome: TestOutcome::Failed,
                        code: invalid_test.code.clone(),
                        was_invalid: true,
                    });
                    return false;
                }
            } else {
                assert_eq!(
                    Some(data),
                    violation.data.as_ref(),
                    "Didn't get expected data for code {:#?}, got: {violation:#?}",
                    invalid_test.code,
                );
            }
        }

        true
    }
}

pub fn compare_ranges(a: Range, b: Range) -> Ordering {
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

#[derive(Builder, Debug)]
#[builder(setter(strip_option, into))]
pub struct RuleTestValid {
    pub code: String,
    #[builder(default)]
    pub options: Option<RuleOptions>,
    #[builder(default)]
    pub only: Option<bool>,
}

impl RuleTestValid {
    pub fn new(code: impl Into<String>, options: Option<RuleOptions>, only: Option<bool>) -> Self {
        Self {
            code: code.into(),
            options,
            only,
        }
    }
}

impl From<&str> for RuleTestValid {
    fn from(value: &str) -> Self {
        Self::new(value, None, None)
    }
}

impl From<String> for RuleTestValid {
    fn from(value: String) -> Self {
        Self::new(value, None, None)
    }
}

#[derive(Builder, Clone)]
#[builder(setter(strip_option, into))]
pub struct RuleTestInvalid {
    pub code: String,
    pub errors: RuleTestExpectedErrors,
    #[builder(default)]
    pub output: Option<RuleTestExpectedOutput>,
    #[builder(default)]
    pub options: Option<RuleOptions>,
    #[builder(default)]
    pub only: Option<bool>,
}

impl RuleTestInvalid {
    pub fn new(
        code: impl Into<String>,
        errors: impl Into<RuleTestExpectedErrors>,
        output: Option<impl Into<RuleTestExpectedOutput>>,
        options: Option<RuleOptions>,
        only: Option<bool>,
    ) -> Self {
        Self {
            code: code.into(),
            errors: errors.into(),
            output: output.map(Into::into),
            options,
            only,
        }
    }
}

#[derive(Clone)]
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

#[derive(Clone)]
pub enum RuleTestExpectedErrors {
    NumErrors(usize),
    Errors(Vec<RuleTestExpectedError>),
}

impl From<usize> for RuleTestExpectedErrors {
    fn from(value: usize) -> Self {
        Self::NumErrors(value)
    }
}

impl From<Vec<RuleTestExpectedError>> for RuleTestExpectedErrors {
    fn from(value: Vec<RuleTestExpectedError>) -> Self {
        Self::Errors(value)
    }
}

#[derive(Builder, Clone, Debug, Default)]
#[builder(default, setter(strip_option))]
pub struct RuleTestExpectedError {
    #[builder(setter(into))]
    pub message: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    #[builder(setter(into))]
    pub type_: Option<String>,
    #[builder(setter(into))]
    pub message_id: Option<String>,
    #[builder(setter(into))]
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

enum TestOutcome {
    Passed,
    Failed,
}

struct TestResult {
    outcome: TestOutcome,
    code: String,
    was_invalid: bool,
}

#[derive(Clone, Default)]
pub struct DummyFromFileRunContextInstanceProvider<'a> {
    _phantom_data: PhantomData<&'a ()>,
}

impl<'a> FromFileRunContextInstanceProvider<'a> for DummyFromFileRunContextInstanceProvider<'a> {
    fn get(
        &self,
        _type_id: TypeId,
        _file_run_context: FileRunContext<'a, '_>,
    ) -> Option<&dyn Tid<'a>> {
        unreachable!()
    }
}

pub struct DummyFromFileRunContextInstanceProviderFactory;

impl FromFileRunContextInstanceProviderFactory for DummyFromFileRunContextInstanceProviderFactory {
    fn create<'a>(&self) -> Box<dyn FromFileRunContextInstanceProvider<'a> + 'a> {
        Box::<DummyFromFileRunContextInstanceProvider<'_>>::default()
    }
}
