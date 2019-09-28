//! If you have a collection of singletons you need to initialize, and some of those singletons
//! are dependent on handles from other singletons, the `InitTree` is the structure you need.
//! After implementing the `Init` trait for all of your singleton types all you need to do is add
//! them to the InitTree, and then call `.init()` to receive all of your initialized types.

#![cfg_attr(feature = "nightly", feature(intrinsics))]

use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use itertools::join;

/// If your dependency tree goes beyond this many layers deep we'll refuse to initialize it.
pub const MAX_TREE_DEPTH: u32 = 500;

/// A tree of types to initialize.
#[derive(Default, Clone)]
pub struct InitTree {
    uninitialized: Vec<TypeInitDef>,
}

/// Largely an implementation detail. However you may need to create one of these if you're manually
/// implementing `Init`.
#[derive(Clone)]
pub struct TypeInitDef {
    id: TypeId,
    deps: &'static dyn Fn() -> Vec<TypeInitDef>,
    deep_deps: &'static dyn Fn(&mut Vec<TypeInitDef>, u32),
    init: &'static dyn Fn(&mut HashMap<TypeId, Box<dyn Any>>) -> Box<dyn Any>,
    name: &'static str,
}

impl TypeInitDef {
    /// Creates a new instance of this type.
    ///
    /// # Arguments
    ///
    /// id: The TypeId for the type this will be able to construct.
    ///
    /// deps: A function returning the list of direct dependencies for the constructed type.
    ///
    /// deep_deps: A function which will populate an empty vector with all dependencies for the
    /// constructed type. Called recursively on dependencies.
    ///
    /// init: A function that retrieves the needed dependencies from a HashMap, initializes the
    /// type, and then returns the instance in a type erased `Box`.
    ///
    /// (nightly only) name: The name of the type this constructs. Usually populated with the
    /// `type_name` intrinsic.
    pub fn new(
        id: TypeId,
        deps: &'static dyn Fn() -> Vec<TypeInitDef>,
        deep_deps: &'static dyn Fn(&mut Vec<TypeInitDef>, u32),
        init: &'static dyn Fn(&mut HashMap<TypeId, Box<dyn Any>>) -> Box<dyn Any>,
        name: &'static str,
    ) -> Self {
        Self {
            id,
            deps,
            deep_deps,
            init,
            name,
        }
    }
}

impl InitTree {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add<T: 'static + Init>(&mut self) {
        self.uninitialized.push(T::self_def());
        let mut deps = Vec::new();
        T::deep_deps_list(&mut deps, 0);
        for t in deps {
            self.uninitialized.push(t)
        }
    }

    pub fn init(mut self) -> InitializedTree {
        let mut initialized = HashMap::new();
        self.uninitialized.sort_by_key(|t| t.id);
        self.uninitialized.dedup_by_key(|t| t.id);
        while self.init_cycle(&mut initialized) > 0 {}
        if !self.uninitialized.is_empty() {
            panic!(
                "Unable to resolve initialization tree. Locked on [{}]",
                join(self.uninitialized.iter().map(|t| t.name), ", ")
            );
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

/// A collection of all the structures after they've been initialized. Call `.take::<MyType>()` on
/// this to obtain the newly initialized structure.
#[derive(Default)]
pub struct InitializedTree(HashMap<TypeId, Box<dyn Any>>);

impl InitializedTree {
    /// Removes the initialized structure from this tree and returns it.
    pub fn take<T: 'static>(&mut self) -> Option<T> {
        self.0
            .remove(&TypeId::of::<T>())
            .map(|v| *v.downcast::<T>().unwrap())
    }

    /// Removes the initialized structure from this tree and returns it. Prefer `take()` if possible,
    /// but this function is provided in case the type can't be determined at compile time.
    pub fn take_by_type_id(&mut self, t: TypeId) -> Option<Box<dyn Any>> {
        self.0.remove(&t)
    }
}

pub trait Init: Sized {
    fn init(initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> Self;
    fn self_def() -> TypeInitDef;
    fn deps_list() -> Vec<TypeInitDef>;
    fn deep_deps_list(t: &mut Vec<TypeInitDef>, call_depth: u32);
}

impl<T: 'static +  Default> Init for T {
    fn init(_: &mut HashMap<TypeId, Box<dyn Any>>) -> Self {
        Default::default()
    }

    fn self_def() -> TypeInitDef {
        TypeInitDef {
            id: TypeId::of::<Self>(),
            deps: &Self::deps_list,
            deep_deps: &Self::deep_deps_list,
            init: &|h| Box::new(Self::init(h)),
            name: "<Unknown Type>", // Default type should never have name printed.
        }
    }

    fn deps_list() -> Vec<TypeInitDef> {
        vec![]
    }

    fn deep_deps_list(_t: &mut Vec<TypeInitDef>, _call_depth: u32) {}
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

            fn self_def() -> TypeInitDef {
                TypeInitDef {
                    id: TypeId::of::<Self>(),
                    deps: &Self::deps_list,
                    deep_deps: &Self::deep_deps_list,
                    init: &|h| Box::new(Self::init(h)),
                    name: stringify!($t),
                }
            }

            fn deps_list() -> Vec<TypeInitDef> {
                vec![$(TypeInitDef {
                    id: TypeId::of::<$arg_type>(),
                    deps: &<$arg_type as Init>::deps_list,
                    deep_deps: &<$arg_type as Init>::deep_deps_list,
                    init: &|h| Box::new(<$arg_type as Init>::init(h)),
                    name: stringify!($arg_type),
                },)*]
            }

            fn deep_deps_list(t: &mut Vec<TypeInitDef>, call_depth: u32) {
                if call_depth >= MAX_TREE_DEPTH {
                    panic!(
                        "Dependency tree too deep, this is usually due to a circular dependency. Current tree: [{}]",
                        join(t.iter().map(|t| t.name), ", ")
                    );
                }
                let direct_deps = <Self as Init>::deps_list();
                for d in direct_deps {
                    t.push(d);
                }
                $(
                    <$arg_type as Init>::deep_deps_list(t, call_depth + 1);
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
