//! # Unrealscript Debugger
//! Module for the Unrealscript debugger interface.
//! See <https://docs.unrealengine.com/udk/Three/DebuggerInterface.html>
//!
//! Unreal controls the lifetime this library, and does not provide much of
//! any error handling or recovery mechanisms. If any of the expected invariants
//! of this interface are violated we will simply panic.
//!
//! The functions in this interface are thin wrappers that simply pass their
//! arguments on to corresponding methods on the debugger state instance.
#![warn(missing_docs)]

use std::{future::Future, sync::{atomic::AtomicBool, Condvar, Mutex}};

use common::Version;
use debugger::Debugger;
use flexi_logger::LoggerHandle;
use futures::{executor::LocalPool, task::LocalSpawnExt};
use pkg_version::{pkg_version_major, pkg_version_minor, pkg_version_patch};
pub mod api;
pub mod debugger;
pub mod lifetime;
pub mod stackhack;

/// A single-threaded runtime for executing futures.
pub struct SingleThreadedRuntime {
    pool: LocalPool,
}

unsafe impl Sync for SingleThreadedRuntime {}

static RUNTIME: Option<SingleThreadedRuntime> = None;
static GAME_RUNTIME: Option<SingleThreadedRuntime> = None;
static GAME_RUNTIME_IN_BREAK: AtomicBool = AtomicBool::new(false);
static GAME_RUNTIME_PENDING_COMMANDS:Mutex<Vec<Box<dyn FnOnce() + Send  + 'static>>> = Mutex::new(Vec::new());
/// The debugger state. Calls from Unreal are dispatched into this instance.
static DEBUGGER: Mutex<Option<Debugger>> = Mutex::new(None);
static LOGGER: Mutex<Option<LoggerHandle>> = Mutex::new(None);
static VARIABLE_REQUST_CONDVAR: Condvar = Condvar::new();
static INTERFACE_VERSION: Version = Version {
    major: pkg_version_major!(),
    minor: pkg_version_minor!(),
    patch: pkg_version_patch!(),
};

/// Adds a command to be executed on the game runtime.
///
/// Commands are queued and executed later when [`consume_game_runtime_pending_commands`] is called.
/// This is useful for deferring work that needs to run on the game runtime thread.
pub fn add_game_runtime_pending_command<F: FnOnce() + Send  + 'static>(f: F) {
    let mut commands = GAME_RUNTIME_PENDING_COMMANDS.lock().unwrap();
    commands.push(Box::new(f));
    log::trace!("Added pending command, queue size: {}", commands.len());
}

/// Schedules an async task to run on the game runtime.
///
/// The task is queued as a pending command and will be spawned on the game runtime
/// when [`consume_game_runtime_pending_commands`] is called.
pub fn add_game_runtime_async_task<F:Future<Output=()> + Send + 'static>(f: F) {
    add_game_runtime_pending_command(move || {
        get_game_runtime_mut().spawn(f);
    });
}

/// Executes all pending commands that have been queued for the game runtime.
///
/// This should be called periodically to process commands added via
/// [`add_game_runtime_pending_command`] and [`add_game_runtime_async_task`].
pub fn consume_game_runtime_pending_commands() {
    let mut commands = GAME_RUNTIME_PENDING_COMMANDS.lock().unwrap();
    let count = commands.len();
    if count > 0 {
        log::trace!("Consuming {} pending commands", count);
    }
    for command in commands.drain(..) {
        command();
    }
}
/// Gets a mutable reference to the main runtime option.
///
/// # Safety
///
/// This function uses `transmute` to convert an immutable static reference to a mutable one.
/// This is safe because the static is only accessed from Unreal's single-threaded context.
pub fn get_runtime_option_mut() -> &'static mut Option<SingleThreadedRuntime> {
    #[allow(mutable_transmutes)]
    unsafe {
        std::mem::transmute(&RUNTIME)
    }
}

/// Gets a mutable reference to the game runtime option.
///
/// # Safety
///
/// This function uses `transmute` to convert an immutable static reference to a mutable one.
/// This is safe because the static is only accessed from Unreal's single-threaded context.
pub fn get_game_runtime_option_mut() -> &'static mut Option<SingleThreadedRuntime> {
    #[allow(mutable_transmutes)]
    unsafe {
        std::mem::transmute(&GAME_RUNTIME)
    }
}

/// Gets a mutable reference to the main runtime.
///
/// # Panics
///
/// Panics if the runtime has not been initialized via [`init_runtime`].
pub fn get_runtime_mut() -> &'static mut SingleThreadedRuntime {
    get_runtime_option_mut()
        .as_mut()
        .expect("Runtime not initialized")
}

/// Gets a mutable reference to the game runtime.
///
/// # Panics
///
/// Panics if the game runtime has not been initialized via [`init_game_runtime`].
pub fn get_game_runtime_mut() -> &'static mut SingleThreadedRuntime {
    get_game_runtime_option_mut()
        .as_mut()
        .expect("Game runtime not initialized")
}

/// Initializes the main runtime.
///
/// # Panics
///
/// Panics if the runtime has already been initialized.
pub fn init_runtime() {
    assert!(RUNTIME.is_none());
    let runtime = SingleThreadedRuntime {
        pool: LocalPool::new(),
    };
    get_runtime_option_mut().replace(runtime);
}

/// Returns whether the game runtime has been initialized.
pub fn game_runtime_is_initialized() -> bool {
    GAME_RUNTIME.is_some()
}

/// Initializes the game runtime.
///
/// # Panics
///
/// Panics if the game runtime has already been initialized.
pub fn init_game_runtime() {
    assert!(GAME_RUNTIME.is_none());
    let runtime = SingleThreadedRuntime {
        pool: LocalPool::new(),
    };
    get_game_runtime_option_mut().replace(runtime);
}

/// Returns whether the game is currently in a break state.
pub fn is_game_runtime_in_break() -> bool {
    GAME_RUNTIME_IN_BREAK.load(std::sync::atomic::Ordering::SeqCst)
}

/// Sets whether the game is currently in a break state.
pub fn set_game_runtime_in_break(in_break: bool) {
    GAME_RUNTIME_IN_BREAK.store(in_break, std::sync::atomic::Ordering::SeqCst)
}

impl SingleThreadedRuntime {
    /// Runs all tasks in the runtime until no more progress can be made.
    ///
    /// This processes all spawned futures until they are either complete or blocked.
    pub fn tick(&mut self) {
        self.pool.run_until_stalled();
    }

    /// Spawns a future on this runtime.
    ///
    /// The future will be executed on this single-threaded runtime.
    ///
    /// # Panics
    ///
    /// Panics if the spawner fails to spawn the future.
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        self.pool.spawner().spawn_local(future).unwrap();
    }

    /// Runs the runtime until the given future completes.
    ///
    /// Returns the output of the future.
    pub fn run_until<F: core::future::Future>(&mut self, future: F) -> F::Output {
        self.pool.run_until(future)
    }
}
