use std::{marker::PhantomData, mem, sync::OnceLock};

use better_any::TidAble;

use crate::FileRunContext;

pub trait FromFileRunContextInstanceProvider<'a>: Sized {
    type Parent: FromFileRunContextInstanceProviderFactory<Provider<'a> = Self>;

    fn get<T: FromFileRunContext<'a> + TidAble<'a>>(
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

mod _sealed {
    use super::*;

    pub trait Sealed<'a> {}

    impl<'a, T1: FromFileRunContext<'a>> Sealed<'a> for (T1,) {}
}

pub trait FromFileRunContextProvidedTypes<'a>: _sealed::Sealed<'a> {
    type OnceLockStorage: FromFileRunContextProvidedTypesOnceLockStorage<'a>;

    // fn len(&self) -> usize;
    // fn get_index<'a, T: FromFileRunContext<'a>>(&self) -> Option<usize>;
    fn once_lock_storage() -> Self::OnceLockStorage;
}

impl<'a, T1> FromFileRunContextProvidedTypes<'a> for (T1,)
// where
//     T1: FromFileRunContext<'a> + for<'b> TidAble<'b>,
where
    T1: FromFileRunContext<'a> + TidAble<'a>,
{
    type OnceLockStorage = FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1>;

    fn once_lock_storage() -> Self::OnceLockStorage {
        FromFileRunContextProvidedTypesOnceLockStorageEnum::One(Default::default(), PhantomData)
    }
}

pub trait FromFileRunContextProvidedTypesOnceLockStorage<'a> {
    // fn get<T: FromFileRunContext<'a> + for<'b> TidAble<'b>>(
    fn get<T: FromFileRunContext<'a> + TidAble<'a>>(
        &self,
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Option<&T>;
}

pub enum FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1> {
    One(OnceLock<T1>, PhantomData<&'a ()>),
}

impl<'a, T1> FromFileRunContextProvidedTypesOnceLockStorage<'a>
    for FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1>
// where
//     T1: FromFileRunContext<'a> + for<'b> TidAble<'b>,
where
    T1: FromFileRunContext<'a> + TidAble<'a>,
{
    // fn get<T: FromFileRunContext<'a> + for<'b> TidAble<'b>>(
    fn get<T: FromFileRunContext<'a> + TidAble<'a>>(
        &self,
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Option<&T> {
        match self {
            FromFileRunContextProvidedTypesOnceLockStorageEnum::One(t1, _) => match T::id() {
                id if id == T1::id() => Some(unsafe {
                    mem::transmute::<&T1, &T>(
                        t1.get_or_init(|| T1::from_file_run_context(file_run_context)),
                    )
                }),
                _ => None,
            },
        }
    }
}
