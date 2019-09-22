//! If you have a collection of singletons you need to initialize, and some of those singletons
//! are dependent on handles from other singletons, the `InitTree` is the structure you need.
//! After implementing the `Init` trait for all of your singleton types all you need to do is add
//! them to the InitTree, and then call `.init()` to receive all of your initialized types.
//!

#![cfg_attr(feature = "nightly", feature(intrinsics))]

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

#[cfg(feature = "nightly")]
extern "rust-intrinsic" {
    fn type_name<T>() -> &'static str;
}

#[cfg(feature = "nightly")]
use itertools::join;

/// A tree of types to initialize.
#[derive(Default, Clone)]
pub struct InitTree {
    uninitialized: Vec<TypeInitDef>,
}

#[derive(Clone)]
pub struct TypeInitDef {
    id: TypeId,
    deps: &'static dyn Fn() -> Vec<TypeInitDef>,
    deep_deps: &'static dyn Fn(&mut Vec<TypeInitDef>),
    init: &'static dyn Fn(&mut HashMap<TypeId, Box<dyn Any>>) -> Box<dyn Any>,
    #[cfg(feature = "nightly")]
    name: &'static str,
}

impl InitTree {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add<T: 'static + Init>(&mut self) {
        self.uninitialized.push(TypeInitDef {
            id: TypeId::of::<T>(),
            deps: &T::deps_list,
            deep_deps: &T::deep_deps_list,
            init: &|h| Box::new(T::init(h)),
            #[cfg(feature = "nightly")]
            name: type_name::<T>(),
        });
        let mut deps = Vec::new();
        T::deep_deps_list(&mut deps);
        for t in deps {
            self.uninitialized.push(t)
        }
    }

    pub fn init(mut self) -> InitializedTree {
        let mut initialized = HashMap::new();
        self.uninitialized.sort_by_key(|t| t.id);
        self.uninitialized.dedup_by_key(|t| t.id);
        while self.init_cycle(&mut initialized) > 0 {}
        if self.uninitialized.len() > 0 {
            #[cfg(not(feature = "nightly"))]
            {
                panic!("Unable to resolve initialization tree. If you need more info please use the nightly feature on the init_tree crate with a nightly compiler.");
            }
            #[cfg(feature = "nightly")]
            {
                let type_list = join(self.uninitialized.iter().map(|t| t.name), ", ");
                panic!(
                    "Unable to resolve initialization tree. Locked on [{}]",
                    type_list
                );
            }
        }
        InitializedTree(initialized)
    }

    fn init_cycle(&mut self, initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> u32 {
        let mut initialized_count = 0;
        let mut i = 0;
        while i < self.uninitialized.len() {
            if (self.uninitialized[i].deps)()
                .iter()
                .all(|t| initialized.contains_key(&t.id))
            {
                let new_init = self.uninitialized.remove(i);
                let new_value = (new_init.init)(initialized);
                initialized.insert(new_init.id, new_value);
                initialized_count += 1;
            } else {
                i += 1;
            }
        }
        initialized_count
    }
}

#[derive(Default)]
pub struct InitializedTree(HashMap<TypeId, Box<dyn Any>>);

impl InitializedTree {
    pub fn take<T: 'static>(&mut self) -> Option<T> {
        self.0
            .remove(&TypeId::of::<T>())
            .map(|v| *v.downcast::<T>().unwrap())
    }

    pub fn take_by_type_id(&mut self, t: &TypeId) -> Option<Box<dyn Any>> {
        self.0.remove(t)
    }
}

pub trait Init: Sized {
    fn init(initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> Self;
    fn deps_list() -> Vec<TypeInitDef>;
    fn deep_deps_list(t: &mut Vec<TypeInitDef>);
}

impl<T: Default> Init for T {
    fn init(_: &mut HashMap<TypeId, Box<dyn Any>>) -> Self {
        Default::default()
    }

    fn deps_list() -> Vec<TypeInitDef> {
        vec![]
    }

    fn deep_deps_list(_t: &mut Vec<TypeInitDef>) {}
}

#[macro_export]
macro_rules! impl_init {
    ($t:ty; ($($arg:ident: &mut $arg_type:ty),*) $init:block) => {
        impl $crate::Init for $t
        {
            fn init(initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> Self {
                $(
                    let mut $arg = initialized.remove(&TypeId::of::<$arg_type>()).unwrap();
                )*
                let ret;
                {
                    $(
                        let $arg = $arg.downcast_mut::<$arg_type>().unwrap();
                    )*
                    ret = $init;
                }

                $(
                    initialized.insert(TypeId::of::<$arg_type>(), $arg);
                )*
                ret
            }

            fn deps_list() -> Vec<TypeInitDef> {
                vec![$(TypeInitDef {
                    id: TypeId::of::<$arg_type>(),
                    deps: &<$arg_type as Init>::deps_list,
                    deep_deps: &<$arg_type as Init>::deep_deps_list,
                    init: &|h| Box::new(<$arg_type as Init>::init(h)),
                    #[cfg(feature = "nightly")]
                    name: type_name::<$arg_type>(),
                },)*]
            }

            fn deep_deps_list(t: &mut Vec<TypeInitDef>) {
                if t.iter().filter(|t| t.id == TypeId::of::<Self>()).count() > 1 {
                    #[cfg(not(feature = "nightly"))]
                    {
                        panic!("Circular InitTree dependency detected! If you need more info please use the nightly feature on the init_tree crate with a nightly compiler.")
                    }
                    #[cfg(feature = "nightly")]
                    {
                        let type_list = join(t
                            .iter()
                            .filter(|t| t.id != TypeId::of::<Self>())
                            .map(|t| t.name), ", ");
                        panic!("Circular InitTree dependency detected! {} has a circular dependency with one of [{}]", type_name::<Self>(), type_list);
                    }
                }
                let direct_deps = <Self as Init>::deps_list();
                for d in direct_deps {
                    t.push(d);
                }
                $(
                    <$arg_type as Init>::deep_deps_list(t);
                )*
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(PartialEq, Eq, Debug)]
    struct InitA {
        b: i32,
        c: i32,
        d: i32,
    }

    impl_init!(InitA; (b: &mut InitB, c: &mut InitC, d: &mut InitD) {
        InitA {
            b: b.get_handle(),
            c: c.get_handle(),
            d: d.0,
        }
    });

    #[derive(Default, PartialEq, Eq, Debug)]
    struct InitB;

    impl InitB {
        fn get_handle(&self) -> i32 {
            5
        }
    }

    #[derive(Default, PartialEq, Eq, Debug)]
    struct InitC;

    impl InitC {
        fn get_handle(&self) -> i32 {
            7
        }
    }

    #[derive(PartialEq, Eq, Debug)]
    struct InitD(i32);

    impl_init!(InitD; (e: &mut InitE) {
        InitD(e.get_handle())
    });

    #[derive(Default, PartialEq, Eq, Debug)]
    struct InitE;

    impl InitE {
        fn get_handle(&self) -> i32 {
            10
        }
    }

    #[test]
    fn test_basic_init() {
        let mut tree = InitTree::new();
        tree.add::<InitA>();
        let mut vals = tree.init();
        assert_eq!(vals.take::<InitA>(), Some(InitA { b: 5, c: 7, d: 10 }));
        assert_eq!(vals.take::<InitB>(), Some(InitB));
        assert_eq!(vals.take::<InitC>(), Some(InitC));
        assert_eq!(vals.take::<InitD>(), Some(InitD(10)));
        assert_eq!(vals.take::<InitE>(), Some(InitE));
    }

    struct CantInitA;

    impl_init!(CantInitA; (_b: &mut CantInitB) {
        CantInitA
    });

    struct CantInitB;

    impl_init!(CantInitB; (_a: &mut CantInitA) {
        CantInitB
    });

    #[test]
    #[should_panic]
    fn test_panic() {
        let mut tree = InitTree::new();
        tree.add::<CantInitA>();
        tree.init();
    }

    #[derive(Default, PartialEq, Eq, Debug)]
    struct BaseCoreInit;

    #[derive(PartialEq, Eq, Debug)]
    struct CoreInit;

    impl_init!(CoreInit; (_base: &mut BaseCoreInit) {
        CoreInit
    });

    #[derive(PartialEq, Eq, Debug)]
    struct LevelOneInit;

    impl_init!(LevelOneInit; (_core: &mut CoreInit) {
        LevelOneInit
    });

    #[derive(PartialEq, Eq, Debug)]
    struct LevelTwoInit;

    impl_init!(LevelTwoInit; (_core: &mut CoreInit, _one: &mut LevelOneInit) {
        LevelTwoInit
    });

    #[derive(PartialEq, Eq, Debug)]
    struct LevelThreeInit;

    impl_init!(LevelThreeInit; (_core: &mut CoreInit, _two: &mut LevelTwoInit) {
        LevelThreeInit
    });

    #[derive(PartialEq, Eq, Debug)]
    struct LevelFourInit;

    impl_init!(LevelFourInit; (_core: &mut CoreInit, _three: &mut LevelThreeInit) {
        LevelFourInit
    });

    #[test]
    fn test_layers_with_shared_dep() {
        let mut tree = InitTree::new();
        tree.add::<LevelFourInit>();
        tree.add::<LevelThreeInit>();
        tree.add::<LevelTwoInit>();
        tree.add::<LevelOneInit>();
        let mut initialized = tree.init();
        assert_eq!(initialized.take::<CoreInit>(), Some(CoreInit));
        assert_eq!(initialized.take::<LevelOneInit>(), Some(LevelOneInit));
        assert_eq!(initialized.take::<LevelTwoInit>(), Some(LevelTwoInit));
        assert_eq!(initialized.take::<LevelThreeInit>(), Some(LevelThreeInit));
        assert_eq!(initialized.take::<LevelFourInit>(), Some(LevelFourInit));
    }
}
