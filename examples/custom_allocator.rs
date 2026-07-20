//! Does calling an already-compiled Steel script allocate?

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use steel::steel_vm::engine::Engine;
use steel::steel_vm::register_fn::RegisterFn;
use steel::SteelVal;

use bumpalo::Bump;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static PANIC_ON_ALLOCATION: AtomicBool = AtomicBool::new(false);

thread_local! {
    // Reporting the first bad allocation (unwinding, formatting the message,
    // capturing a backtrace) itself allocates. Without this guard, that nested
    // allocation trips the same check and Rust aborts with "thread panicked
    // while processing panic" before anything useful gets printed.
    static REPORTING: Cell<bool> = const { Cell::new(false) };
}

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if PANIC_ON_ALLOCATION.load(Relaxed) && !REPORTING.with(|r| r.replace(true)) {
            panic!("unexpected allocation: {layout:?}");
        }
        ALLOCS.fetch_add(1, Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if PANIC_ON_ALLOCATION.load(Relaxed) && !REPORTING.with(|r| r.replace(true)) {
            panic!("unexpected dealloc: {layout:?}");
        }
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    // Warm the thread-local now, so its own first-touch init can't be mistaken
    // for a call-path allocation once PANIC_ON_ALLOCATION is armed below.
    REPORTING.with(|_| {});

    let mut bump_alloc = Bump::with_capacity(1024);

    let mut engine = Engine::new();
    engine.register_fn("emit-note", |note: u8, velocity: u8| {
        println!("note={note} velocity={velocity}");
    });

    engine
        .run("(define (on-note note velocity) (emit-note note velocity))")
        .unwrap();

    const CALLS: u64 = 10_000;
    let before = ALLOCS.load(Relaxed);
    PANIC_ON_ALLOCATION.store(true, std::sync::atomic::Ordering::Relaxed);
    for _ in 0..CALLS {
        let mut args = [SteelVal::IntV(60), SteelVal::IntV(100)];
        engine
            .call_function_by_name_with_args_from_mut_slice_in("on-note", &mut args, &bump_alloc)
            .unwrap();
        bump_alloc.reset();
    }
    PANIC_ON_ALLOCATION.store(false, Relaxed);
    let allocs = ALLOCS.load(Relaxed) - before;

    println!(
        "{allocs} allocations over {CALLS} calls ({:.2} per call)",
        allocs as f64 / CALLS as f64
    );
}
