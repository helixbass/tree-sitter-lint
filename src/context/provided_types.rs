use std::{any::TypeId, marker::PhantomData, sync::OnceLock};

use better_any::{Tid, TidAble};

use crate::FileRunContext;

pub trait FromFileRunContextInstanceProvider<'a>: Sized {
    type Parent: FromFileRunContextInstanceProviderFactory<Provider<'a> = Self>;

    fn get(
        &self,
        type_id: TypeId,
        file_run_context: FileRunContext<'a, '_, Self::Parent>,
    ) -> Option<&dyn Tid<'a>>;
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
    fn get(
        &self,
        type_id: TypeId,
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Option<&dyn Tid<'a>>;
}

pub enum FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1> {
    One(OnceLock<T1>, PhantomData<&'a ()>),
}

impl<'a, T1> FromFileRunContextProvidedTypesOnceLockStorage<'a>
    for FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1>
where
    T1: FromFileRunContext<'a> + TidAble<'a>,
{
    fn get(
        &self,
        type_id: TypeId,
        file_run_context: FileRunContext<'a, '_, impl FromFileRunContextInstanceProviderFactory>,
    ) -> Option<&dyn Tid<'a>> {
        match self {
            FromFileRunContextProvidedTypesOnceLockStorageEnum::One(t1, _) => match type_id {
                id if id == T1::id() => {
                    Some(t1.get_or_init(|| T1::from_file_run_context(file_run_context)))
                }
                _ => None,
            },
        }
    }
}
