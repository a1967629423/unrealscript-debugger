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

use std::sync::{atomic::AtomicBool, Condvar, Mutex};

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
static GAME_RUNTIME_IN_BREAK:AtomicBool = AtomicBool::new(false);
/// The debugger state. Calls from Unreal are dispatched into this instance.
static DEBUGGER: Mutex<Option<Debugger>> = Mutex::new(None);
static LOGGER: Mutex<Option<LoggerHandle>> = Mutex::new(None);
static VARIABLE_REQUST_CONDVAR: Condvar = Condvar::new();
static INTERFACE_VERSION: Version = Version {
    major: pkg_version_major!(),
    minor: pkg_version_minor!(),
    patch: pkg_version_patch!(),
};

/// TODO
pub fn get_runtime_option_mut() -> &'static mut Option<SingleThreadedRuntime> {
    #[allow(mutable_transmutes)]
    unsafe  { std::mem::transmute(&RUNTIME) }
}
/// TODO
pub fn get_game_runtime_option_mut() -> &'static mut Option<SingleThreadedRuntime> {
    #[allow(mutable_transmutes)]
    unsafe  { std::mem::transmute(&GAME_RUNTIME) }
}
/// TODO
pub fn get_runtime_mut() -> &'static mut SingleThreadedRuntime {
    get_runtime_option_mut().as_mut().expect("Runtime not initialized")
}
/// TODO
pub fn get_game_runtime_mut() -> &'static mut SingleThreadedRuntime {
    get_game_runtime_option_mut().as_mut().expect("Game runtime not initialized")
}
/// TODO
pub fn init_runtime() {
    assert!(RUNTIME.is_none());
    let runtime = SingleThreadedRuntime {
        pool: LocalPool::new(),
    };
    get_runtime_option_mut().replace(runtime);
}

/// TODO
pub fn game_runtime_is_initialized() -> bool {
    GAME_RUNTIME.is_some()
}
/// TODO
pub fn init_game_runtime() {
    assert!(GAME_RUNTIME.is_none());
    let runtime = SingleThreadedRuntime {
        pool: LocalPool::new(),
    };
    get_game_runtime_option_mut().replace(runtime);
}
/// TODO
pub fn is_game_runtime_in_break() -> bool {
    GAME_RUNTIME_IN_BREAK.load(std::sync::atomic::Ordering::Relaxed)
}
/// TODO
pub fn set_game_runtime_in_break(in_break: bool) {
    GAME_RUNTIME_IN_BREAK.store(in_break, std::sync::atomic::Ordering::Relaxed)
}

impl SingleThreadedRuntime {
    /// TODO
    pub fn tick(&mut self) {
        self.pool.run_until_stalled();
    }
    /// TODO
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + 'static
    {
        self.pool.spawner().spawn_local(future).unwrap();
    }
    /// TODO
    pub fn run_until<F: core::future::Future>(&mut self, future: F) -> F::Output {
        self.pool.run_until(future)
    }
}