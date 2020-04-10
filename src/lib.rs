//! If you have a collection of singletons you need to initialize, and some of those singletons
//! are dependent on handles from other singletons, the `InitTree` is the structure you need.
//! After implementing the `Init` trait for all of your singleton types all you need to do is add
//! them to the InitTree, and then call `.init()` to receive all of your initialized types.

use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
};

#[cfg(feature = "cache")]
use std::mem::swap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Forwards compatible, serde compatible, opaque cache structure. Used to cache initialization sequences.
/// Caching can be disabled by turning off the default features for this crate.
#[cfg(feature = "cache")]
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Cache {
    inner: CacheVersion,
}

#[cfg(feature = "cache")]
#[derive(Clone, Debug, Deserialize, Serialize)]
enum CacheVersion {
    V1(Vec<usize>),
}

#[cfg(feature = "cache")]
impl Default for Cache {
    fn default() -> Self {
        Self {
            inner: CacheVersion::V1(Vec::new()),
        }
    }
}

/// A tree of types to initialize.
#[derive(Default, Clone)]
pub struct InitTree {
    uninitialized: Vec<internal::TypeInitDef>,
    #[cfg(feature = "cache")]
    cache: Option<Cache>,
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
    #[cfg(feature = "cache")]
    pub fn enable_caching(&mut self, enabled: bool) {
        if enabled {
            if self.cache.is_none() {
                self.cache = Some(Cache::default());
            }
        } else {
            self.cache = None;
        }
    }

    /// Loads a new cache in, returning the old one, if any.
    /// This will automatically enable caching.
    #[cfg(feature = "cache")]
    pub fn load_cache(&mut self, cache: Cache) -> Option<Cache> {
        let prior = self.cache.take();
        self.cache = Some(cache);
        prior
    }

    /// Request that this tree initialize the provided type T
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
        #[cfg(feature = "cache")]
        let mut cache_was_correct = self.cache.is_some();
        #[cfg(feature = "cache")]
        {
            if let Some(Cache {
                inner: CacheVersion::V1(cache),
            }) = self.cache.as_mut()
            {
                // This cache may be invalid. So we're going to replace it
                // with whatever we learn from this run.
                let mut new_cache = Vec::new();
                for i in cache.iter() {
                    let mut new_init = self.uninitialized.swap_remove(*i);
                    if let Some(new_value) = (new_init.init)(&mut initialized) {
                        initialized.insert((new_init.id)(), RefCell::new(new_value));
                        new_cache.push(*i);
                    } else {
                        // If we couldn't initialize the value undo the swap_remove
                        if self.uninitialized.len() > *i {
                            swap(&mut self.uninitialized[*i], &mut new_init);
                        }
                        self.uninitialized.push(new_init);
                    }
                }
                *cache = new_cache;
            }
        }
        if self.init_cycle(&mut initialized) > 0 {
            #[cfg(feature = "cache")]
            {
                cache_was_correct = false;
            }

            while self.init_cycle(&mut initialized) > 0 {}
        }
        if !self.uninitialized.is_empty() {
            panic!(
                "Unable to resolve initialization tree. Locked on [{}]",
                internal::get_type_names(self.uninitialized.iter())
            );
        }
        InitializedTree {
            tree: initialized
                .into_iter()
                .map(|(k, v)| (k, v.into_inner()))
                .collect(),

            #[cfg(feature = "cache")]
            cache: self.cache,
            #[cfg(feature = "cache")]
            cache_was_correct,
        }
    }

    fn init_cycle(&mut self, initialized: &mut HashMap<TypeId, RefCell<Box<dyn Any>>>) -> u32 {
        let mut initialized_count = 0;
        let mut i = 0;
        while i < self.uninitialized.len() {
            if (self.uninitialized[i].deps)()
                .iter()
                .all(|t| initialized.contains_key(&(t.id)()))
            {
                let new_init = self.uninitialized.swap_remove(i);
                if let Some(new_value) = (new_init.init)(initialized) {
                    initialized.insert((new_init.id)(), RefCell::new(new_value));
                    initialized_count += 1;
                    #[cfg(feature = "cache")]
                    {
                        if let Some(Cache {
                            inner: CacheVersion::V1(cache),
                        }) = self.cache.as_mut()
                        {
                            cache.push(i);
                        }
                    }
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
    #[cfg(feature = "cache")]
    cache: Option<Cache>,
    #[cfg(feature = "cache")]
    cache_was_correct: bool,
}

impl InitializedTree {
    /// Removes the initialized structure from this tree and returns it.
    pub fn take<T: 'static>(&mut self) -> Option<T> {
        self.tree
            .remove(&TypeId::of::<T>())
            .map(|v| *v.downcast::<T>().unwrap())
    }

    /// Returns an iterator of all initialized types.
    pub fn take_all(self) -> impl Iterator<Item = (TypeId, Box<dyn Any>)> {
        self.tree.into_iter()
    }

    /// Removes the initialized structure from this tree and returns it. Prefer `take()` if possible,
    /// but this function is provided in case the type can't be determined at compile time.
    pub fn take_by_type_id(&mut self, t: TypeId) -> Option<Box<dyn Any>> {
        self.tree.remove(&t)
    }

    /// Return the cache from this initialization.
    #[cfg(feature = "cache")]
    pub fn take_cache(&mut self) -> Option<Cache> {
        self.cache.take()
    }

    /// Returns true if the cache loaded in was completely correct.
    #[cfg(feature = "cache")]
    pub fn cache_was_correct(&self) -> bool {
        self.cache_was_correct
    }
}

/// The trait that must be implemented for a type before it can be used with the `InitTree`.
/// Automatically implemented for `Default` types.
///
/// You are discouraged from implementing this manually, and should use the `impl_init` macro
/// instead.
pub trait Init: Sized {
    fn init(initialized: &mut HashMap<TypeId, RefCell<Box<dyn Any>>>) -> Option<Self>;
    fn self_def() -> internal::TypeInitDef;
    fn deps_list() -> &'static [internal::TypeInitDef];
    fn deep_deps_list(t: &mut Vec<internal::TypeInitDef>, call_depth: u32);
}

impl<T: 'static + Default> Init for T {
    fn init(_: &mut HashMap<TypeId, RefCell<Box<dyn Any>>>) -> Option<Self> {
        Some(Default::default())
    }

    fn self_def() -> internal::TypeInitDef {
        internal::TypeInitDef {
            id: TypeId::of::<Self>,
            deps: Self::deps_list,
            init: |h| Self::init(h).map(|h| Box::new(h) as Box<dyn Any>),
            name: "<Unknown Type>", // Default type should never have name printed.
        }
    }

    fn deps_list() -> &'static [internal::TypeInitDef] {
        &[]
    }

    fn deep_deps_list(_t: &mut Vec<internal::TypeInitDef>, _call_depth: u32) {}
}

/// Provides an impl of the `Init` trait for a type.
///
/// This is structured roughly as a function definition. The only acceptable args for it are
/// mutable references to other structures with an `Init` or `Default` implementation.
///
/// # Example
///
/// ```
/// # use init_tree::impl_init;
///#[derive(Default, PartialEq, Eq, Debug)]
/// struct InitDependency;
///
/// #[derive(PartialEq, Eq, Debug)]
/// struct InitMe;
///
/// impl_init!(InitMe; (_dep: &mut InitDependency) {
///     InitMe
/// });
/// ```
#[macro_export]
macro_rules! impl_init {
    ($t:ty; ($($arg:ident: &mut $arg_type:ty),*) $init:block) => {
        impl $crate::Init for $t
        {
            fn init(initialized: &mut std::collections::HashMap<std::any::TypeId, std::cell::RefCell<Box<dyn std::any::Any>>>) -> Option<Self> {
                $(
                    let mut $arg = initialized.get(&std::any::TypeId::of::<$arg_type>())?.borrow_mut();
                    let $arg = $arg.downcast_mut::<$arg_type>().unwrap();
                )*
                Some($init)
            }

            fn self_def() -> $crate::internal::TypeInitDef {
                $crate::internal::TypeInitDef {
                    id: std::any::TypeId::of::<Self>,
                    deps: Self::deps_list,
                    init: |h| Self::init(h).map(|h| Box::new(h) as Box<dyn std::any::Any>),
                    name: stringify!($t),
                }
            }

            #[allow(non_upper_case_globals)]
            fn deps_list() -> &'static [$crate::internal::TypeInitDef] {
                $(const $arg: $crate::internal::TypeInitDef = $crate::internal::TypeInitDef {
                    id: std::any::TypeId::of::<$arg_type>,
                    deps: <$arg_type as $crate::Init>::deps_list,
                    init: |h| <$arg_type as $crate::Init>::init(h).map(|h| Box::new(h) as Box<dyn std::any::Any>),
                    name: stringify!($arg_type),
                };)*
                &[$($arg,)*]
            }

            fn deep_deps_list(t: &mut Vec<$crate::internal::TypeInitDef>, call_depth: u32) {
                if call_depth >= $crate::internal::MAX_TREE_DEPTH {
                    panic!(
                        "Dependency tree too deep, this is usually due to a circular dependency. Current tree: [{}]",
                        $crate::internal::get_type_names(t.iter())
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

/// These items are required to be public for macros, but their direct use is discouraged.
pub mod internal {
    use std::{
        any::{Any, TypeId},
        cell::RefCell,
        collections::HashMap,
    };

    use itertools::join;

    /// If your dependency tree goes beyond this many layers deep we'll refuse to initialize it.
    pub const MAX_TREE_DEPTH: u32 = 500;

    /// Largely an implementation detail. However you may need to create one of these if you're manually
    /// implementing `Init`.
    #[derive(Clone, Copy)]
    pub struct TypeInitDef {
        pub id: fn() -> TypeId,
        pub deps: fn() -> &'static [TypeInitDef],
        pub init: fn(&mut HashMap<TypeId, RefCell<Box<dyn Any>>>) -> Option<Box<dyn Any>>,
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
        /// init: A function that retrieves the needed dependencies from a HashMap, initializes the
        /// type, and then returns the instance in a type erased `Box`. May return `None` if not all
        /// dependencies were available.
        ///
        /// name: The name of the type this constructs.
        pub fn new(
            id: fn() -> TypeId,
            deps: fn() -> &'static [TypeInitDef],
            init: fn(&mut HashMap<TypeId, RefCell<Box<dyn Any>>>) -> Option<Box<dyn Any>>,
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
    pub fn get_type_names<'a>(defs: impl Iterator<Item = &'a TypeInitDef>) -> String {
        join(defs.map(|d| d.name), ", ")
    }
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

    #[derive(PartialEq, Eq, Debug)]
    struct SelfDep;

    impl_init!(SelfDep; (_me: &mut SelfDep) {
        SelfDep
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
    #[cfg(feature = "cache")]
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

    #[test]
    #[should_panic]
    fn test_self_referential_init() {
        let mut tree = InitTree::new();
        tree.add::<SelfDep>();
        let mut init = tree.init();
        assert_eq!(init.take::<SelfDep>(), Some(SelfDep));
    }

    #[test]
    fn test_compile_fail() {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/ui/shouldnt_compile.rs");
    }
}
