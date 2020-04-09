//! If you have a collection of singletons you need to initialize, and some of those singletons
//! are dependent on handles from other singletons, the `InitTree` is the structure you need.
//! After implementing the `Init` trait for all of your singleton types all you need to do is add
//! them to the InitTree, and then call `.init()` to receive all of your initialized types.

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
    cache: Option<Vec<usize>>,
}

/// Largely an implementation detail. However you may need to create one of these if you're manually
/// implementing `Init`.
#[derive(Clone, Copy)]
pub struct TypeInitDef {
    pub id: fn() -> TypeId,
    pub deps: fn() -> &'static [TypeInitDef],
    pub init: fn(&mut HashMap<TypeId, Box<dyn Any>>) -> Box<dyn Any>,
    pub name: &'static str,
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
    /// name: The name of the type this constructs.
    pub fn new(
        id: fn() -> TypeId,
        deps: fn() -> &'static [TypeInitDef],
        init: fn(&mut HashMap<TypeId, Box<dyn Any>>) -> Box<dyn Any>,
        name: &'static str,
    ) -> Self {
        Self {
            id,
            deps,
            init,
            name,
        }
    }
}

/// Here for use in macros. Returns a comma separated string of the type names in this iterator.
pub fn get_type_names<'a>(defs: impl Iterator<Item=&'a TypeInitDef>) -> String {
    join(defs.map(|d| d.name), ", ")
}

impl InitTree {
    pub fn new() -> Self {
        Default::default()
    }

    /// InitTree supports the use of a cache between initializations.
    /// It's common for the initialization sequence to be identical between runs.
    /// If you find that the process of discovering dependencies is slowing down
    /// your initialization you can cache the results of this discovery to make future
    /// initializations faster.
    ///
    /// One might for example serialize the cache to a file after initialization and load
    /// it from that file when initializing in the future.
    pub fn enable_caching(&mut self, enabled: bool) {
        if enabled {
            if self.cache == None {
                self.cache = Some(Vec::new());
            }
        } else {
            self.cache = None;
        }
    }

    /// Loads a new cache in, returning the old one, if any.
    /// This will automatically enable caching.
    pub fn load_cache(&mut self, cache: Vec<usize>) -> Option<Vec<usize>> {
        let prior = self.cache.take();
        self.cache = Some(cache);
        prior
    }
    
    pub fn add<T: 'static + Init>(&mut self) {
        self.uninitialized.push(T::self_def());
        T::deep_deps_list(&mut self.uninitialized, 0);
    }

    /// Initializes the tree, returning a fully initialized tree, and if caching is enabled, a
    /// cache from this initialization.
    pub fn init(mut self) -> InitializedTree {
        let mut initialized = HashMap::new();
        self.uninitialized.sort_by_key(|t| t.id);
        self.uninitialized.dedup_by_key(|t| t.id);
        let mut cache_was_correct = self.cache.is_some();
        if let Some(cache) = &mut self.cache {
            // This cache may be invalid. So we're going to replace it
            // with whatever we learn from this run.
            let mut new_cache = Vec::new();
            for i in cache.iter() {
                if (self.uninitialized[*i].deps)()
                    .iter()
                    .all(|t| initialized.contains_key(&(t.id)()))
                {
                    let new_init = self.uninitialized.swap_remove(*i);
                    let new_value = (new_init.init)(&mut initialized);
                    initialized.insert((new_init.id)(), new_value);
                    new_cache.push(*i);
                } else {
                    cache_was_correct = false;
                }
            }
            *cache = new_cache;
        }
        while self.init_cycle(&mut initialized) > 0 {
            cache_was_correct = false;
        }
        if !self.uninitialized.is_empty() {
            panic!(
                "Unable to resolve initialization tree. Locked on [{}]",
                get_type_names(self.uninitialized.iter()) 
            );
        }
        InitializedTree {
            tree: initialized,
            cache: self.cache,
            cache_was_correct,
        }
    }

    fn init_cycle(&mut self, initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> u32 {
        let mut initialized_count = 0;
        let mut i = 0;
        while i < self.uninitialized.len() {
            if (self.uninitialized[i].deps)()
                .iter()
                .all(|t| initialized.contains_key(&(t.id)()))
            {
                let new_init = self.uninitialized.swap_remove(i);
                let new_value = (new_init.init)(initialized);
                initialized.insert((new_init.id)(), new_value);
                initialized_count += 1;
                if let Some(cache) = &mut self.cache {
                    cache.push(i);
                }
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
pub struct InitializedTree {
    tree: HashMap<TypeId, Box<dyn Any>>,
    cache: Option<Vec<usize>>,
    cache_was_correct: bool,
}

impl InitializedTree {
    /// Removes the initialized structure from this tree and returns it.
    pub fn take<T: 'static>(&mut self) -> Option<T> {
        self.tree
            .remove(&TypeId::of::<T>())
            .map(|v| *v.downcast::<T>().unwrap())
    }

    /// Removes the initialized structure from this tree and returns it. Prefer `take()` if possible,
    /// but this function is provided in case the type can't be determined at compile time.
    pub fn take_by_type_id(&mut self, t: TypeId) -> Option<Box<dyn Any>> {
        self.tree.remove(&t)
    }

    /// Return the cache from this initialization.
    pub fn take_cache(&mut self) -> Option<Vec<usize>> {
        self.cache.take()
    }

    /// Returns true if the cache loaded in was completely correct.
    pub fn cache_was_correct(&self) -> bool {
       self.cache_was_correct 
    }
}

pub trait Init: Sized {
    fn init(initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> Self;
    fn self_def() -> TypeInitDef;
    fn deps_list() -> &'static [TypeInitDef];
    fn deep_deps_list(t: &mut Vec<TypeInitDef>, call_depth: u32);
}

impl<T: 'static + Default> Init for T {
    fn init(_: &mut HashMap<TypeId, Box<dyn Any>>) -> Self {
        Default::default()
    }

    fn self_def() -> TypeInitDef {
        TypeInitDef {
            id: TypeId::of::<Self>,
            deps: Self::deps_list,
            init: |h| Box::new(Self::init(h)),
            name: "<Unknown Type>", // Default type should never have name printed.
        }
    }

    fn deps_list() -> &'static [TypeInitDef] {
        &[]
    }

    fn deep_deps_list(_t: &mut Vec<TypeInitDef>, _call_depth: u32) {}
}

#[macro_export]
macro_rules! impl_init {
    ($t:ty; ($($arg:ident: &mut $arg_type:ty),*) $init:block) => {
        impl $crate::Init for $t
        {
            fn init(initialized: &mut std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>>) -> Self {
                $(
                    let mut $arg = initialized.remove(&std::any::TypeId::of::<$arg_type>()).unwrap();
                )*
                let ret;
                {
                    $(
                        let $arg = $arg.downcast_mut::<$arg_type>().unwrap();
                    )*
                    ret = $init;
                }

                $(
                    initialized.insert(std::any::TypeId::of::<$arg_type>(), $arg);
                )*
                ret
            }

            fn self_def() -> $crate::TypeInitDef {
                $crate::TypeInitDef {
                    id: std::any::TypeId::of::<Self>,
                    deps: Self::deps_list,
                    init: |h| Box::new(Self::init(h)),
                    name: stringify!($t),
                }
            }

            #[allow(non_upper_case_globals)]
            fn deps_list() -> &'static [$crate::TypeInitDef] {
                $(const $arg: $crate::TypeInitDef = $crate::TypeInitDef {
                    id: std::any::TypeId::of::<$arg_type>,
                    deps: <$arg_type as $crate::Init>::deps_list,
                    init: |h| Box::new(<$arg_type as $crate::Init>::init(h)),
                    name: stringify!($arg_type),
                };)*
                &[$($arg,)*]
            }

            fn deep_deps_list(t: &mut Vec<$crate::TypeInitDef>, call_depth: u32) {
                if call_depth >= $crate::MAX_TREE_DEPTH {
                    panic!(
                        "Dependency tree too deep, this is usually due to a circular dependency. Current tree: [{}]",
                        $crate::get_type_names(t.iter())
                    );
                }
                t.extend(Self::deps_list().iter());
                $(
                    <$arg_type as $crate::Init>::deep_deps_list(t, call_depth + 1);
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
        let mut vals= tree.init();
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

    #[test]
    fn test_caching() {
        fn test_init() -> InitTree {
            let mut tree = InitTree::new();
            tree.add::<LevelFourInit>();
            tree.add::<LevelThreeInit>();
            tree.add::<LevelTwoInit>();
            tree.add::<LevelOneInit>();
            tree
        }
        let mut init = test_init();
        init.enable_caching(true);
        let mut initialized = init.init();
        let cache = initialized.take_cache();
        assert!(!initialized.cache_was_correct());
        let mut init = test_init();
        init.load_cache(cache.unwrap());
        let mut initialized = init.init();
        assert!(initialized.take_cache().is_some());
        assert!(initialized.cache_was_correct());
    }
}
