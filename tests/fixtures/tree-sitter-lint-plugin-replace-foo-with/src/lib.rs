use std::sync::Arc;

use tree_sitter_lint::{
    better_any::tid, rule, tree_sitter_grep::RopeOrSlice, violation, FileRunContext,
    FromFileRunContext, FromFileRunContextInstanceProviderFactory, Plugin, Rule,
};

// pub type ProvidedTypes<'a> = ();
pub type ProvidedTypes<'a> = (Foo<'a>,);

#[derive(Clone)]
pub struct Foo<'a> {
    #[allow(dead_code)]
    text: &'a str,
}

impl<'a> FromFileRunContext<'a> for Foo<'a> {
    fn from_file_run_context(
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Self {
        println!("instantiating Foo for {:#?}", file_run_context.path);
        Self {
            text: match &file_run_context.file_contents {
                RopeOrSlice::Slice(file_contents) => {
                    std::str::from_utf8(&file_contents[..4]).unwrap()
                }
                _ => unreachable!(),
            },
        }
    }
}

tid! { impl<'a> TidAble<'a> for Foo<'a> }

// #[derive(Default)]
// struct FooProvider<'a> {
//     foo_instance: OnceLock<Foo<'a>>,
// }

// impl<'a> FromFileRunContextInstanceProvider<'a> for FooProvider<'a> {
//     type Parent = FooProviderFactory;

//     fn get<T: FromFileRunContext<'a> + for<'b> TidAble<'b>>(
//         &self,
//         file_run_context: FileRunContext<'a, '_, Self::Parent>,
//     ) -> Option<&T> {
//         match T::id() {
//             id if id == Foo::<'a>::id() => Some(unsafe {
//                 mem::transmute::<&Foo<'a>, &T>(
//                     self.foo_instance
//                         .get_or_init(|| Foo::from_file_run_context(file_run_context)),
//                 )
//             }),
//             _ => None,
//         }
//     }
// }

// struct FooProviderFactory;

// impl FromFileRunContextInstanceProviderFactory for FooProviderFactory {
//     type Provider<'a> = FooProvider<'a>;

//     fn create<'a>(&self) -> Self::Provider<'a> {
//         FooProvider {
//             foo_instance: Default::default(),
//         }
//     }
// }

pub fn instantiate<T: FromFileRunContextInstanceProviderFactory>() -> Plugin<T> {
    Plugin {
        name: "replace-foo-with".to_owned(),
        rules: vec![
            replace_foo_with_bar_rule(),
            replace_foo_with_something_rule(),
            starts_with_use_rule(),
        ],
    }
}

fn replace_foo_with_bar_rule<T: FromFileRunContextInstanceProviderFactory>() -> Arc<dyn Rule<T>> {
    rule! {
        name => "replace-foo-with-bar",
        fixable => true,
        listeners => [
            r#"(
              (identifier) @c (#eq? @c "foo")
            )"# => |node, context| {
                context.report(
                    violation! {
                        node => node,
                        message => r#"Use 'bar' instead of 'foo'"#,
                        fix => |fixer| {
                            fixer.replace_text(node, "bar");
                        }
                    }
                );
            }
        ],
        languages => [Rust]
    }
}

fn replace_foo_with_something_rule<T: FromFileRunContextInstanceProviderFactory>(
) -> Arc<dyn Rule<T>> {
    rule! {
        name => "replace-foo-with-something",
        fixable => true,
        options_type => String,
        state => {
            [per-run]
            replacement: String = options,
        },
        listeners => [
            r#"(
              (identifier) @c (#eq? @c "foo")
            )"# => |node, context| {
                context.report(
                    violation! {
                        node => node,
                        message => format!(r#"Use '{}' instead of 'foo'"#, self.replacement),
                        fix => |fixer| {
                            fixer.replace_text(node, &self.replacement);
                        }
                    }
                );
            }
        ],
        languages => [Rust]
    }
}

fn starts_with_use_rule<T: FromFileRunContextInstanceProviderFactory>() -> Arc<dyn Rule<T>> {
    rule! {
        name => "starts-with-use",
        listeners => [
            r#"
              (use_declaration) @c
            "# => |node, context| {
                if context.retrieve::<Foo<'a>>().text == "use " {
                    context.report(
                        violation! {
                            node => node,
                            message => r#"Starts with 'use'"#,
                        }
                    );
                }
            }
        ],
        languages => [Rust]
    }
}
