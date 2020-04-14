use init_tree::impl_init;

#[derive(PartialEq, Eq, Debug)]
struct NoInit;

#[derive(PartialEq, Eq, Debug)]
struct NeedsNoInit;

impl_init!(NeedsNoInit; (_no_init: &mut NoInit) {
    NeedsNoInit
});

fn main() {

}
