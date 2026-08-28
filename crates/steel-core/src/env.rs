use crate::gc::{Allocator, Global};
use crate::rvals::Result;
use crate::rvals::SteelValGeneric;
use shared_vector::AtomicSharedVector;

// `Env`'s global bindings storage is cfg-split the same way `Gc<T>`/`Gc<T, A>` is (see
// gc.rs): the `sync+biased+allocator-api2` combo is the only one where a `Gc<T, A>` with a
// non-`Global` `A` actually exists, so it's the only one where the bindings buffer's own
// backing memory is routed through `A` (`AtomicSharedVector<SteelValGeneric<A>, A>`). Every
// other combo still stores `SteelValGeneric<A>` as the element type -- just backed by
// `shared_vector`'s own default allocator, since routing the buffer itself through `A` needs
// `allocator-api2` specifically.
#[cfg(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
))]
pub(crate) struct SharedVectorWrapper<A: Allocator + Clone + Send + Sync + 'static = Global>(
    pub AtomicSharedVector<SteelValGeneric<A>, A>,
);

#[cfg(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
))]
impl<A: Allocator + Clone + Send + Sync + 'static> Clone for SharedVectorWrapper<A> {
    fn clone(&self) -> Self {
        SharedVectorWrapper(self.0.clone())
    }
}

#[cfg(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
))]
impl<A: Allocator + Clone + Send + Sync + 'static> core::fmt::Debug for SharedVectorWrapper<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
))]
impl<A: Allocator + Clone + Send + Sync + 'static> SharedVectorWrapper<A> {
    pub fn set_idx(&mut self, idx: usize, val: SteelValGeneric<A>) -> SteelValGeneric<A> {
        let guard = self.0.get_mut(idx).unwrap();
        let output = guard.clone();
        *guard = val;
        output
    }

    pub fn repl_define_idx(&mut self, idx: usize, val: SteelValGeneric<A>) {
        let guard = &mut self.0;
        if idx < guard.len() {
            guard[idx] = val.clone();
        } else {
            if idx > guard.len() {
                // if idx > self.thread_local_bindings.len() {
                // TODO: This seems suspect. Try to understand
                // what is happening here. This would be that values
                // are getting interned to be at a global offset in the
                // wrong order, which seems to be fine in general,
                // assuming that the values then get actually updated
                // to the correct values.
                for _ in 0..(idx - guard.len()) {
                    guard.push(SteelValGeneric::Void);
                }
            }

            guard.push(val.clone());
        }
    }

    /// A fresh, empty buffer using the same allocator as `self` -- used as a cheap
    /// placeholder to swap out the live bindings during `with_locked_env` (vm.rs). Building
    /// it from `self`'s own allocator instance (rather than a shared `Global` static, as the
    /// non-generic version used) means this works for any `A`, not just ones that are
    /// `Default`-constructible.
    pub fn empty_like(&self) -> Self {
        let alloc = self.0.allocator().clone();
        SharedVectorWrapper(AtomicSharedVector::with_capacity_in(0, alloc))
    }
}

// Safety: mirrors the non-generic version's assumption -- `Env`'s bindings are only ever
// mutated under the VM's safepoint/synchronizer protocol (vm.rs), never through unsynchronized
// concurrent access, regardless of what `A` is.
#[cfg(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
))]
unsafe impl<A: Allocator + Clone + Send + Sync + 'static> Sync for SharedVectorWrapper<A> {}

#[cfg(not(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
)))]
#[derive(educe::Educe)]
#[educe(Debug, Clone)]
pub(crate) struct SharedVectorWrapper<A: Allocator + Clone + Send + Sync + 'static = Global>(
    pub AtomicSharedVector<SteelValGeneric<A>>,
);

#[cfg(not(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
)))]
impl<A: Allocator + Clone + Send + Sync + 'static> SharedVectorWrapper<A> {
    pub fn set_idx(&mut self, idx: usize, val: SteelValGeneric<A>) -> SteelValGeneric<A> {
        let guard = self.0.get_mut(idx).unwrap();
        let output = guard.clone();
        *guard = val;
        output
    }

    pub fn repl_define_idx(&mut self, idx: usize, val: SteelValGeneric<A>) {
        let guard = &mut self.0;
        if idx < guard.len() {
            guard[idx] = val.clone();
        } else {
            if idx > guard.len() {
                // if idx > self.thread_local_bindings.len() {
                // TODO: This seems suspect. Try to understand
                // what is happening here. This would be that values
                // are getting interned to be at a global offset in the
                // wrong order, which seems to be fine in general,
                // assuming that the values then get actually updated
                // to the correct values.
                for _ in 0..(idx - guard.len()) {
                    guard.push(SteelValGeneric::Void);
                }
            }

            guard.push(val.clone());
        }
    }
}

#[cfg(not(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
)))]
unsafe impl<A: Allocator + Clone + Send + Sync + 'static> Sync for SharedVectorWrapper<A> {}

#[derive(educe::Educe)]
#[educe(Debug, Clone)]
pub struct Env<A: Allocator + Clone + Send + Sync + 'static = Global> {
    #[cfg(not(feature = "sync"))]
    pub(crate) bindings_vec: Vec<SteelValGeneric<A>>,

    #[cfg(feature = "sync")]
    pub(crate) bindings: SharedVectorWrapper<A>,
}

#[cfg(not(feature = "sync"))]
impl<A: Allocator + Clone + Send + Sync + 'static> Env<A> {
    pub fn extract(&self, idx: usize) -> Option<SteelValGeneric<A>> {
        self.bindings_vec.get(idx).cloned()
    }

    pub fn len(&self) -> usize {
        self.bindings_vec.len()
    }

    /// top level global env has no parent -- `alloc` is only used by the gated combo's
    /// storage, but every combo needs the same constructor signature so callers (vm.rs) don't
    /// need to know which one they're building against.
    pub fn root_in(alloc: A) -> Self {
        let _ = alloc;
        Env {
            bindings_vec: Vec::with_capacity(1024),
        }
    }

    #[cfg(feature = "dynamic")]
    pub(crate) fn _print_diagnostics(&self) {
        for (idx, value) in self.bindings_vec.iter().enumerate() {
            if let SteelValGeneric::Closure(b) = value {
                let count = b.call_count();
                if count > 0 {
                    println!("Function: {} - Count: {}", idx, b.call_count());
                }
            }
        }
    }

    #[inline(always)]
    pub fn repl_lookup_idx(&self, idx: usize) -> SteelValGeneric<A> {
        self.bindings_vec[idx].clone()
    }

    #[inline(always)]
    pub fn repl_maybe_lookup_idx(&self, idx: usize) -> Option<SteelValGeneric<A>> {
        // Look up the bindings using the local copy
        self.bindings_vec.get(idx).cloned()
    }

    /// Get the value located at that index
    pub fn _repl_get_idx(&self, idx: usize) -> &SteelValGeneric<A> {
        &self.bindings_vec[idx]
    }

    #[inline]
    pub fn repl_define_idx(&mut self, idx: usize, val: SteelValGeneric<A>) {
        if idx < self.bindings_vec.len() {
            self.bindings_vec[idx] = val;
        } else {
            if idx > self.bindings_vec.len() {
                // TODO: This seems suspect. Try to understand
                // what is happening here. This would be that values
                // are getting interned to be at a global offset in the
                // wrong order, which seems to be fine in general,
                // assuming that the values then get actually updated
                // to the correct values.
                for _ in 0..(idx - self.bindings_vec.len()) {
                    self.bindings_vec.push(SteelValGeneric::Void);
                }
            }

            self.bindings_vec.push(val);
            assert_eq!(self.bindings_vec.len() - 1, idx);
        }
    }

    pub fn repl_set_idx(&mut self, idx: usize, val: SteelValGeneric<A>) -> Result<SteelValGeneric<A>> {
        let output = self.bindings_vec[idx].clone();
        self.bindings_vec[idx] = val;
        Ok(output)
    }

    #[inline]
    pub fn add_root_value(&mut self, idx: usize, val: SteelValGeneric<A>) {
        // self.bindings_map.insert(idx, val);
        self.repl_define_idx(idx, val);
    }

    pub fn roots(&self) -> &Vec<SteelValGeneric<A>> {
        &self.bindings_vec
    }
}

#[cfg(all(
    feature = "sync",
    not(all(
        feature = "biased",
        feature = "allocator-api2",
        not(feature = "triomphe")
    ))
))]
impl<A: Allocator + Clone + Send + Sync + 'static> Env<A> {
    pub fn len(&self) -> usize {
        self.bindings.0.len()
    }

    /// top level global env has no parent -- `alloc` is only used by the gated combo's
    /// storage, but every combo needs the same constructor signature so callers (vm.rs) don't
    /// need to know which one they're building against.
    pub fn root_in(alloc: A) -> Self {
        let _ = alloc;
        Env {
            bindings: SharedVectorWrapper(AtomicSharedVector::with_capacity(1024)),
        }
    }

    pub fn deep_clone(&self) -> Self {
        Self {
            bindings: SharedVectorWrapper(
                self.bindings.clone().0.into_unique().into_shared_atomic(),
            ),
        }
    }

    #[inline(always)]
    pub fn repl_lookup_idx(&self, idx: usize) -> SteelValGeneric<A> {
        // Look up the bindings using the local copy
        self.bindings.0[idx].clone()
    }

    #[inline(always)]
    pub fn repl_maybe_lookup_idx(&self, idx: usize) -> Option<SteelValGeneric<A>> {
        // Look up the bindings using the local copy
        self.bindings.0.get(idx).cloned()
    }

    #[inline]
    pub fn update_env(&mut self, vec: SharedVectorWrapper<A>) {
        self.bindings = vec;
    }

    #[inline]
    pub(crate) fn default_env(&mut self) {
        // `SharedVectorWrapper<A>` can't live in a `static` for arbitrary `A` (statics can't
        // be generic), so unlike the concrete version this used to be, there's no shared
        // empty-buffer cache to reuse here -- just build a fresh empty one.
        self.bindings = SharedVectorWrapper(shared_vector::arc_vector!());
    }

    pub(crate) fn drain_env(&mut self) -> SharedVectorWrapper<A> {
        let output = self.bindings.clone();
        self.default_env();
        output
    }

    #[inline(always)]
    pub fn repl_set_idx(&mut self, idx: usize, val: SteelValGeneric<A>) -> Result<SteelValGeneric<A>> {
        let guard = self.bindings.0.get_mut(idx).unwrap();
        let output = guard.clone();
        *guard = val;
        Ok(output)
    }

    pub fn roots(&self) -> &[SteelValGeneric<A>] {
        self.bindings.0.as_slice()
    }
}

#[cfg(all(
    feature = "sync",
    feature = "biased",
    feature = "allocator-api2",
    not(feature = "triomphe")
))]
impl<A: Allocator + Clone + Send + Sync + 'static> Env<A> {
    pub fn len(&self) -> usize {
        self.bindings.0.len()
    }

    /// top level global env has no parent
    pub fn root_in(alloc: A) -> Self {
        Env {
            bindings: SharedVectorWrapper(AtomicSharedVector::with_capacity_in(1024, alloc)),
        }
    }

    pub fn deep_clone(&self) -> Self {
        Self {
            bindings: SharedVectorWrapper(
                self.bindings.clone().0.into_unique().into_shared_atomic(),
            ),
        }
    }

    #[inline(always)]
    pub fn repl_lookup_idx(&self, idx: usize) -> SteelValGeneric<A> {
        // Look up the bindings using the local copy
        self.bindings.0[idx].clone()
    }

    #[inline(always)]
    pub fn repl_maybe_lookup_idx(&self, idx: usize) -> Option<SteelValGeneric<A>> {
        // Look up the bindings using the local copy
        self.bindings.0.get(idx).cloned()
    }

    #[inline]
    pub fn update_env(&mut self, vec: SharedVectorWrapper<A>) {
        self.bindings = vec;
    }

    #[inline]
    pub(crate) fn default_env(&mut self) {
        self.bindings = self.bindings.empty_like();
    }

    pub(crate) fn drain_env(&mut self) -> SharedVectorWrapper<A> {
        let output = self.bindings.clone();
        self.default_env();
        output
    }

    #[inline(always)]
    pub fn repl_set_idx(&mut self, idx: usize, val: SteelValGeneric<A>) -> Result<SteelValGeneric<A>> {
        let guard = self.bindings.0.get_mut(idx).unwrap();
        let output = guard.clone();
        *guard = val;
        Ok(output)
    }

    pub fn roots(&self) -> &[SteelValGeneric<A>] {
        self.bindings.0.as_slice()
    }
}

