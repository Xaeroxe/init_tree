# init_tree

During a program's initialization process it's really common for some singletons
to be dependent on other singletons. For example, Foo needs a handle to data in Bar
before Foo can be initialized. For really large software projects with many
maintainers, keeping this initialization process straight can get to be a headache.
That's where the init_tree comes in.

At program startup you add all of your singletons to the tree, and then call
`init()` on the tree. It will resolve all of your data dependencies at runtime.
It does so by utilizing a trait implemented on all of your singletons, `Init`.
`Init` is implemented automatically for singletons with `Default` implemented.
The `Init` trait provides a list of dependencies, and a function to initialize the
structure with those dependencies. Additionally, a macro `impl_init!` is provided
in order to make implementing `Init` easy to do.

This crate should be usable as is, however it needs better documentation and more
unit testing.
