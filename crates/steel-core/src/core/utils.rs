// Generates a macro that expands to code snippet to do arity checks
// example usage:
// arity_check!(length, args, 1);
// Expands to:
// if args.len() != 1 {
//     stop!(ArityMismatch => format!("length expected only one argument, found {}", args.len()))
// }
// assert!(args.len() == 1);

macro_rules! arity_check_generator {
    ($($arity:tt),*) => {
        macro_rules! arity_check {
            ($name:tt, $args:expr, 1) => {
                if $args.len() != 1 {
                    stop!(ArityMismatch => format!(stringify!($name expected only one argument, found {}), $args.len()))
                }
                assert!($args.len() == 1);
            };

            $ (
                ($name:tt, $args:expr, $arity) => {
                    if $args.len() != $arity {
                        stop!(ArityMismatch => format!(stringify!($name expected {} arguments, found {}), $arity, $args.len()))
                    }
                    assert!($args.len() == $arity);
                };
            ) *
        }
    }
}

arity_check_generator!(0, 1, 2, 3, 4, 5, 6, 7, 8);

pub(crate) use arity_check;

// Declares a const for a function that takes an immutable slice to the arguments
// e.g.
//      declare_const_ref_functions! { LENGTH => length }
// expands into
//      const LENGTH: SteelVal = SteelVal::FuncV(length)
macro_rules! declare_const_ref_functions {
    ($($name:tt => $func_name:tt),* $(,)? ) => {
        $ (
            pub(crate) const $name: SteelVal = SteelVal::FuncV($func_name);
        ) *
    }
}

pub(crate) use declare_const_ref_functions;

// Declares a const for a function that takes an immutable slice to the arguments
// e.g.
//      declare_const_mut_ref_functions! { CONS => cons }
// expands into
//      const LENGTH: SteelVal = SteelVal::MutFunc(length)
macro_rules! declare_const_mut_ref_functions {
    ($($name:tt => $func_name:tt),* $(,)? ) => {
        $ (
            pub(crate) const $name: SteelVal = SteelVal::MutFunc($func_name);
        ) *
    }
}

pub(crate) use declare_const_mut_ref_functions;

// pub(crate) trait Boxed {
//     fn boxed(self) -> Box<Self>;
//     fn refcounted(self) -> Rc<Self>;
//     fn rc_refcell(self) -> Rc<RefCell<Self>>;
// }

// impl<T> Boxed for T {
//     #[inline(always)]
//     fn boxed(self) -> Box<T> {
//         Box::new(self)
//     }

//     #[inline(always)]
//     fn refcounted(self) -> Rc<T> {
//         Rc::new(self)
//     }

//     #[inline(always)]
//     fn rc_refcell(self) -> Rc<RefCell<T>> {
//         Rc::new(RefCell::new(self))
//     }
// }

/// Minimal `collect_in` support for `allocator_api2::vec::Vec<T, A>`, mirroring
/// `bumpalo::collections::{FromIteratorIn, CollectIn}`. Unlike bumpalo's `Vec<'bump, T>`,
/// `allocator_api2::vec::Vec` has no inherent `from_iter_in` to delegate to, so this has
/// to actually build the vec rather than forwarding to a same-named method.
pub trait FromIteratorIn<T> {
    type Alloc;

    fn from_iter_in<I>(iter: I, alloc: Self::Alloc) -> Self
    where
        I: IntoIterator<Item = T>;
}

#[cfg(not(feature = "nightly"))]
use allocator_api2::alloc::{Allocator, Global};
#[cfg(feature = "nightly")]
use std::alloc::Allocator;

impl<A: Allocator, T> FromIteratorIn<T> for allocator_api2::vec::Vec<T, A> {
    type Alloc = A;

    fn from_iter_in<I>(iter: I, alloc: A) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let iter = iter.into_iter();
        let mut vec = allocator_api2::vec::Vec::with_capacity_in(iter.size_hint().0, alloc);
        vec.extend(iter);
        vec
    }
}

pub trait CollectIn: Iterator + Sized {
    fn collect_in<C: FromIteratorIn<Self::Item>>(self, alloc: C::Alloc) -> C {
        C::from_iter_in(self, alloc)
    }
}

impl<I: Iterator> CollectIn for I {}
