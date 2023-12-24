use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Type};

pub fn instance_provider_factory_with_crate_name(
    input: TokenStream,
    crate_name: &str,
) -> TokenStream {
    let provided_types: Type = parse_macro_input!(input);

    let crate_name = format_ident!("{}", crate_name);

    quote! {{
        struct InstanceProviderFactory;

        impl #crate_name::FromFileRunContextInstanceProviderFactory for InstanceProviderFactory {
            fn create<'a>(&self) -> Box<dyn #crate_name::FromFileRunContextInstanceProvider<'a> + 'a> {
                Box::new(InstanceProvider {
                    provided_types: <#provided_types::<'a> as #crate_name::FromFileRunContextProvidedTypes::<'a>>::once_lock_storage(),
                })
            }
        }

        struct InstanceProvider<'a> {
            provided_types: <#provided_types::<'a> as #crate_name::FromFileRunContextProvidedTypes::<'a>>::OnceLockStorage,
        }

        impl<'a> #crate_name::FromFileRunContextInstanceProvider<'a> for InstanceProvider<'a> {
            fn get(
                &self,
                type_id: std::any::TypeId,
                file_run_context: #crate_name::FileRunContext<'a, '_>,
            ) -> Option<&dyn #crate_name::better_any::Tid<'a>> {
                #crate_name::FromFileRunContextProvidedTypesOnceLockStorage::get(&self.provided_types, type_id, file_run_context)
            }
        }

        InstanceProviderFactory
    }}
    .into()
}
