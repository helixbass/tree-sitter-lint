use better_any::TidAble;

use crate::FileRunContext;

pub trait FromFileRunContextInstanceProvider<'a>: Sized {
    type Parent: FromFileRunContextInstanceProviderFactory<Provider<'a> = Self>;

    fn get<T: FromFileRunContext<'a> + for<'b> TidAble<'b>>(
        &self,
        file_run_context: FileRunContext<'a, '_, Self::Parent>,
    ) -> Option<&T>;
}

pub trait FromFileRunContextInstanceProviderFactory: Send + Sync {
    type Provider<'a>: FromFileRunContextInstanceProvider<'a, Parent = Self>;

    fn create<'a>(&self) -> Self::Provider<'a>;
}

pub trait FromFileRunContext<'a> {
    fn from_file_run_context(
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Self;
}
