use std::{any::{Any, TypeId}, collections::HashMap};

#[derive(Default)]
pub struct InitTree {
    initialized: HashMap<TypeId, Box<dyn Any>>,
    uninitialized: Vec<(TypeId, Box<dyn Fn() -> Vec<TypeId>>, Box<dyn FnOnce(&mut HashMap<TypeId, Box<dyn Any>>) -> Box<dyn Any>>)>,
}

impl InitTree {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add<T: 'static +  Init>(&mut self) {
        self.uninitialized.push((TypeId::of::<T>(), Box::new(T::deps_list), Box::new(|h| Box::new(T::init(h)))));
    }

    pub fn init(mut self) -> HashMap<TypeId, Box<dyn Any>> {
        while self.init_cycle() > 0 {}
        if self.uninitialized.len() > 0 {
            panic!("Unable to resolve initialization tree");
        }
        self.initialized
    }

    fn init_cycle(&mut self) -> u32 {
        let mut initialized = 0;
        let mut i = 0;
        while i < self.uninitialized.len() {
            if self.can_init(&self.uninitialized[i].1()) {
                let new_init = self.uninitialized.remove(i);
                let new_value = new_init.2(&mut self.initialized);
                self.initialized.insert(new_init.0, new_value);
                initialized += 1;
            }
            else {
                i += 1;
            }
        }
        initialized
    }

    fn can_init(&self, deps_list: &[TypeId]) -> bool {
        deps_list.iter().all(|t| self.initialized.contains_key(&t))
    }
}

pub trait Init: Sized {
    fn init(initialized: &mut HashMap<TypeId, Box<dyn Any>>) -> Self;
    fn deps_list() -> Vec<TypeId>;
}

impl<T: Default> Init for T {
    fn init(_: &mut HashMap<TypeId, Box<dyn Any>>) -> Self {
        Default::default()
    }

    fn deps_list() -> Vec<TypeId> {
        vec![]
    }
}

#[macro_export]
macro_rules! impl_init {
    ($t:ty; ($($arg:ident: &mut $arg_type:ty),*) $init:block) => {
        use std::{any::{Any, TypeId}, collections::HashMap};

        impl $crate::Init for $t {
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

            fn deps_list() -> Vec<TypeId> {
                vec![$(TypeId::of::<$arg_type>(),)*]
            }
        }
    };
}


#[cfg(test)]
mod tests {
    use super::*;

    #[derive(PartialEq, Eq, Debug)]
    struct InitA;

    #[derive(Default, PartialEq, Eq, Debug)]
    struct InitB;

    #[derive(Default, PartialEq, Eq, Debug)]
    struct InitC;

    impl_init!(InitA; (_b: &mut InitB, _c: &mut InitC) {
        InitA
    });

    #[test]
    fn test_basic_init() {
        let mut tree = InitTree::new();
        tree.add::<InitA>();
        tree.add::<InitB>();
        tree.add::<InitC>();
        let vals = tree.init();
        assert!(vals.contains_key(&TypeId::of::<InitA>()));
        assert!(vals.contains_key(&TypeId::of::<InitB>()));
        assert!(vals.contains_key(&TypeId::of::<InitC>()));
        assert_eq!(vals.get(&TypeId::of::<InitA>()).unwrap().downcast_ref::<InitA>().unwrap(), &InitA);
        assert_eq!(vals.get(&TypeId::of::<InitB>()).unwrap().downcast_ref::<InitB>().unwrap(), &InitB);
        assert_eq!(vals.get(&TypeId::of::<InitC>()).unwrap().downcast_ref::<InitC>().unwrap(), &InitC);
    }
}
