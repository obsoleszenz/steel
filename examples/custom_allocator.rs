//! Does calling an already-compiled Steel script allocate?

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use steel::rvals::IntoSteelVal;
use steel::steel_vm::builtin::BuiltInModule;
use steel::steel_vm::engine::Engine;
use steel::steel_vm::register_fn::RegisterFn;
use steel::SteelVal;

use bumpalo::Bump;
use steel_derive::Steel;

// steel-core only depends on the `Allocator` trait (from `allocator-api2`) -- it has no
// opinion on, or dependency on, which concrete allocator backs `ArenaAlloc::custom`. Picking
// bumpalo, and making it thread-safe enough to hand over (`Bump` is `Send` but not `Sync`,
// since it's built on `Cell`s), is entirely on us as the embedder.
struct MutexBump(std::sync::Mutex<Bump>);

unsafe impl allocator_api2::alloc::Allocator for MutexBump {
    fn allocate(
        &self,
        layout: Layout,
    ) -> Result<std::ptr::NonNull<[u8]>, allocator_api2::alloc::AllocError> {
        (&*self.0.lock().unwrap()).allocate(layout)
    }

    fn allocate_zeroed(
        &self,
        layout: Layout,
    ) -> Result<std::ptr::NonNull<[u8]>, allocator_api2::alloc::AllocError> {
        (&*self.0.lock().unwrap()).allocate_zeroed(layout)
    }

    unsafe fn deallocate(&self, ptr: std::ptr::NonNull<u8>, layout: Layout) {
        unsafe { (&*self.0.lock().unwrap()).deallocate(ptr, layout) }
    }
}

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

// `constructor` + `getters` auto-generates a constructor for every variant (unit-variant
// constructors are, quirkily, gated behind `getters` rather than `constructor` in the
// current steel-derive impl) plus a `Name-Variant?` predicate for each, all bundled into
// the generated `PlayerCommand::register_enum_variants` associated function below.
#[derive(Clone, Debug, Steel)]
#[steel(constructor, getters)]
pub enum PlayerCommand {
    Play,
    Pause,
    LoopBeats(i64),
}

#[derive(Clone, Debug, Steel)]
#[steel(constructor)]
pub enum Command {
    Player(PlayerCommand),
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    // Warm the thread-local now, so its own first-touch init can't be mistaken
    // for a call-path allocation once PANIC_ON_ALLOCATION is armed below.
    REPORTING.with(|_| {});

    let mut bump_alloc = Bump::with_capacity(1024);

    let mut engine = Engine::new();
    engine.register_fn("push_command", |time: u64, command: Command| {
        println!("time={time} command={command:?}");
    });

    engine
        .run("(define (on-command time command) (push_command time command))")
        .unwrap();

    // One call each registers every predicate/constructor the derive generated,
    // instead of a register_fn per variant.
    let mut module = BuiltInModule::new("player-commands");
    PlayerCommand::register_enum_variants(&mut module);
    Command::register_enum_variants(&mut module);
    engine.register_module(module);

    engine
        .run(
            r#"
            (require-builtin player-commands)
            (define loop-cmd (Command-Player (PlayerCommand-LoopBeats 42)))
            "#,
        )
        .unwrap();

    let loop_cmd: Command = engine.extract("loop-cmd").unwrap();
    println!("loop_cmd = {loop_cmd:?}");

    const CALLS: u64 = 10_000;

    // Built once outside the hot loop: cloning a SteelVal wrapping a Custom value is just
    // a Gc refcount bump, not a fresh allocation, so reusing this keeps the loop alloc-free.
    let command_val = Command::Player(PlayerCommand::LoopBeats(42))
        .into_steelval()
        .unwrap();

    let before = ALLOCS.load(Relaxed);
    PANIC_ON_ALLOCATION.store(true, std::sync::atomic::Ordering::Relaxed);
    for i in 0..CALLS {
        let mut args = [SteelVal::IntV(i as isize), command_val.clone()];
        engine
            .call_function_by_name_with_args_from_mut_slice_in(
                "on-command",
                &mut args,
                &bump_alloc,
            )
            .unwrap();
        bump_alloc.reset();
    }
    PANIC_ON_ALLOCATION.store(false, Relaxed);
    let allocs = ALLOCS.load(Relaxed) - before;

    println!(
        "{allocs} allocations over {CALLS} calls ({:.2} per call)",
        allocs as f64 / CALLS as f64
    );

    // `Gc<T, A>`: the refcount box itself can live in a custom allocator instead of the
    // global one. `BiasedRc`'s cross-thread machinery requires `A: 'static`, so the arena
    // needs a 'static handle -- leaking one for the life of the program is the standard way
    // to get that for a long-lived, thread-confined arena (e.g. one owned by an audio thread).
    // `with_capacity` (unlike `Bump::new`) allocates its first chunk eagerly, so it doesn't
    // surprise PANIC_ON_ALLOCATION with a one-time lazy-init allocation on first use.
    let arena: &'static Bump = Box::leak(Box::new(Bump::with_capacity(1024)));

    let before = ALLOCS.load(Relaxed);
    PANIC_ON_ALLOCATION.store(true, Relaxed);
    let arena_value = steel::gc::Gc::new_in(42i64, arena);
    assert_eq!(*arena_value, 42);
    let cloned = steel::gc::Gc::clone(&arena_value);
    drop(cloned);
    drop(arena_value);
    PANIC_ON_ALLOCATION.store(false, Relaxed);
    let arena_allocs = ALLOCS.load(Relaxed) - before;

    println!(
        "{arena_allocs} global allocations while building/cloning/dropping a Gc<i64, &Bump>"
    );

    // Put it all together: configure this engine's Custom-value allocator to a bump arena
    // (instead of the default Global), then have the Steel-side script build a *fresh* Command
    // value on every single call -- exercising the full path (the outer Gc/RcBox and the inner
    // Box<dyn CustomType>, both routed through the arena) rather than reusing one built ahead
    // of time like the very first loop above did.
    engine.set_custom_value_arena(steel::gc::ArenaAlloc::custom(MutexBump(std::sync::Mutex::new(
        Bump::with_capacity(1024 * 1024),
    ))));

    engine
        .run(
            "(define (on-command-fresh time) \
               (push_command time (Command-Player (PlayerCommand-LoopBeats time))))",
        )
        .unwrap();

    let before = ALLOCS.load(Relaxed);
    PANIC_ON_ALLOCATION.store(true, Relaxed);
    for i in 0..CALLS {
        let mut args = [SteelVal::IntV(i as isize)];
        engine
            .call_function_by_name_with_args_from_mut_slice_in(
                "on-command-fresh",
                &mut args,
                &bump_alloc,
            )
            .unwrap();
        bump_alloc.reset();
    }
    PANIC_ON_ALLOCATION.store(false, Relaxed);
    let fresh_allocs = ALLOCS.load(Relaxed) - before;

    println!(
        "{fresh_allocs} allocations over {CALLS} calls building a fresh Command each time ({:.2} per call)",
        fresh_allocs as f64 / CALLS as f64
    );
}
