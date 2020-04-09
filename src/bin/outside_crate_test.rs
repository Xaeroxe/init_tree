use init_tree::{InitTree, impl_init};

fn main() {
    let mut tree = InitTree::new();
        tree.add::<InitA>();
        let mut vals= tree.init();
        assert_eq!(vals.take::<InitA>(), Some(InitA { b: 5, c: 7, d: 10 }));
        assert_eq!(vals.take::<InitB>(), Some(InitB));
        assert_eq!(vals.take::<InitC>(), Some(InitC));
        assert_eq!(vals.take::<InitD>(), Some(InitD(10)));
        assert_eq!(vals.take::<InitE>(), Some(InitE));

}
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
