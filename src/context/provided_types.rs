use std::{any::TypeId, marker::PhantomData, sync::OnceLock};

use better_any::{tid, Tid, TidAble};

use crate::FileRunContext;

pub trait FromFileRunContextInstanceProvider<'a> {
    fn get(
        &self,
        type_id: TypeId,
        file_run_context: FileRunContext<'a, '_>,
    ) -> Option<&dyn Tid<'a>>;
}

pub trait FromFileRunContextInstanceProviderFactory: Send + Sync {
    fn create<'a>(&self) -> Box<dyn FromFileRunContextInstanceProvider<'a> + 'a>;
}

pub trait FromFileRunContext<'a> {
    fn from_file_run_context(file_run_context: FileRunContext<'a, '_>) -> Self;
}

mod _sealed {
    use super::*;

    pub trait Sealed<'a> {}

    impl<'a> Sealed<'a> for () {}
    impl<'a, T1: FromFileRunContext<'a>> Sealed<'a> for (T1,) {}
    impl<'a, T1: FromFileRunContext<'a>, T2: FromFileRunContext<'a>> Sealed<'a> for (T1, T2) {}
}

pub trait FromFileRunContextProvidedTypes<'a>: _sealed::Sealed<'a> {
    type OnceLockStorage: FromFileRunContextProvidedTypesOnceLockStorage<'a>;

    // fn len(&self) -> usize;
    // fn get_index<'a, T: FromFileRunContext<'a>>(&self) -> Option<usize>;
    fn once_lock_storage() -> Self::OnceLockStorage;
}

impl<'a> FromFileRunContextProvidedTypes<'a> for () {
    type OnceLockStorage = FromFileRunContextProvidedTypesOnceLockStorageEnum<
        'a,
        DummyFromFileRunContext<'a>,
        DummyFromFileRunContext<'a>,
    >;

    fn once_lock_storage() -> Self::OnceLockStorage {
        FromFileRunContextProvidedTypesOnceLockStorageEnum::Zero(PhantomData)
    }
}

impl<'a, T1> FromFileRunContextProvidedTypes<'a> for (T1,)
where
    T1: FromFileRunContext<'a> + TidAble<'a>,
{
    type OnceLockStorage =
        FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1, DummyFromFileRunContext<'a>>;

    fn once_lock_storage() -> Self::OnceLockStorage {
        FromFileRunContextProvidedTypesOnceLockStorageEnum::One(Default::default(), PhantomData)
    }
}

impl<'a, T1, T2> FromFileRunContextProvidedTypes<'a> for (T1, T2)
where
    T1: FromFileRunContext<'a> + TidAble<'a>,
    T2: FromFileRunContext<'a> + TidAble<'a>,
{
    type OnceLockStorage = FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1, T2>;

    fn once_lock_storage() -> Self::OnceLockStorage {
        FromFileRunContextProvidedTypesOnceLockStorageEnum::Two(
            Default::default(),
            Default::default(),
            PhantomData,
        )
    }
}

pub trait FromFileRunContextProvidedTypesOnceLockStorage<'a> {
    fn get(
        &self,
        type_id: TypeId,
        file_run_context: FileRunContext<'a, '_>,
    ) -> Option<&dyn Tid<'a>>;
}

pub enum FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1, T2> {
    Zero(PhantomData<&'a ()>),
    One(OnceLock<T1>, PhantomData<&'a ()>),
    Two(OnceLock<T1>, OnceLock<T2>, PhantomData<&'a ()>),
}

impl<'a, T1, T2> FromFileRunContextProvidedTypesOnceLockStorage<'a>
    for FromFileRunContextProvidedTypesOnceLockStorageEnum<'a, T1, T2>
where
    T1: FromFileRunContext<'a> + TidAble<'a>,
    T2: FromFileRunContext<'a> + TidAble<'a>,
{
    fn get(
        &self,
        type_id: TypeId,
        file_run_context: FileRunContext<'a, '_>,
    ) -> Option<&dyn Tid<'a>> {
        match self {
            FromFileRunContextProvidedTypesOnceLockStorageEnum::Zero(_) => None,
            FromFileRunContextProvidedTypesOnceLockStorageEnum::One(t1, _) => match type_id {
                id if id == T1::id() => {
                    Some(t1.get_or_init(|| T1::from_file_run_context(file_run_context)))
                }
                _ => None,
            },
            FromFileRunContextProvidedTypesOnceLockStorageEnum::Two(t1, t2, _) => match type_id {
                id if id == T1::id() => {
                    Some(t1.get_or_init(|| T1::from_file_run_context(file_run_context)))
                }
                id if id == T2::id() => {
                    Some(t2.get_or_init(|| T2::from_file_run_context(file_run_context)))
                }
                _ => None,
            },
        }
    }
}

pub struct DummyFromFileRunContext<'a> {
    _phantom_data: PhantomData<&'a ()>,
}

impl<'a> FromFileRunContext<'a> for DummyFromFileRunContext<'a> {
    fn from_file_run_context(_file_run_context: FileRunContext<'a, '_>) -> Self {
        unreachable!()
    }
}

tid! { impl<'a> TidAble<'a> for DummyFromFileRunContext<'a> }

// pub fn get_instance_provider_factory_for_provided_types<
//     TProvidedTypes: for<'a> FromFileRunContextProvidedTypes<'a> + Send + Sync,
// >() -> InstanceProviderFactoryForProvidedTypes<TProvidedTypes> {
//     InstanceProviderFactoryForProvidedTypes {
//         _phantom_data: PhantomData,
//     }
// }

// pub struct InstanceProviderFactoryForProvidedTypes<
//     TProvidedTypes: for<'a> FromFileRunContextProvidedTypes<'a> + Send + Sync,
// > {
//     _phantom_data: PhantomData<TProvidedTypes>,
// }

// impl<TProvidedTypes: for<'a> FromFileRunContextProvidedTypes<'a> + Send + Sync + 'static>
//     FromFileRunContextInstanceProviderFactory for InstanceProviderFactoryForProvidedTypes<TProvidedTypes>
// {
//     fn create<'a>(&self) -> Box<dyn FromFileRunContextInstanceProvider<'a> + 'a> {
//         Box::new(InstanceProviderForProvidedTypes::<'a, TProvidedTypes>::new())
//     }
// }

// struct InstanceProviderForProvidedTypes<'a, TProvidedTypes: FromFileRunContextProvidedTypes<'a>> {
//     provided_instances: TProvidedTypes::OnceLockStorage,
// }

// impl<'a, TProvidedTypes: FromFileRunContextProvidedTypes<'a>>
//     InstanceProviderForProvidedTypes<'a, TProvidedTypes>
// {
//     pub fn new() -> Self {
//         Self {
//             provided_instances: TProvidedTypes::once_lock_storage(),
//         }
//     }
// }

// impl<'a, TProvidedTypes: FromFileRunContextProvidedTypes<'a>> FromFileRunContextInstanceProvider<'a>
//     for InstanceProviderForProvidedTypes<'a, TProvidedTypes>
// {
//     fn get(
//         &self,
//         type_id: TypeId,
//         file_run_context: FileRunContext<'a, '_>,
//     ) -> Option<&dyn Tid<'a>> {
//         self.provided_instances.get(type_id, file_run_context)
//     }
// }
