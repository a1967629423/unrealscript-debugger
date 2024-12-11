//! Module for API entry points from Unreal to the debugger interface.
//!
//! See: <https://docs.unrealengine.com/udk/Three/DebuggerInterface.html>
//!
//! This contains all the publicly exported functions defined by the Unrealscript
//! debugger interface.

/// The unreal callback type. Note that the debugger specification defines
/// it as accepting a 'const char*' parameter but we use u8 here. This is
/// for convenience since it is primarily passed strings.
pub type UnrealCallback = extern "C" fn(*const u8) -> ();
/// TODO:
pub type UnrealVADebugCallback = extern "C" fn(i32, LPCWSTR) -> ();

use std::ffi::c_char;

use crate::{
    consume_game_runtime_pending_commands, game_runtime_is_initialized,
    get_game_runtime_option_mut, init_game_runtime,
    lifetime::{initialize, va_initialized},
    set_game_runtime_in_break,
};
use common::WatchKind;
use log;
use winapi::shared::{minwindef::DWORD, ntdef::LPCWSTR};

use crate::DEBUGGER;

/// TODO: This is a list of all the commands that Unreal can send to the debugger interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum VACMD {
    /// Show the DLL form.
    ShowDllForm,
    /// Add a class to the class hierarchy.
    EditorCommand,
    /// Clear the class hierarchy.
    EditorLoadTextBuffer,
    /// Build the class hierarchy.
    AddClassToHierarchy,
    /// Clear the class hierarchy.
    ClearHierarchy,
    /// Build the class hierarchy.
    BuildHierarchy,
    /// Clear the watch list.
    ClearWatch,
    /// Add a watch to the watch list.
    AddWatch,
    /// Unused.
    SetCallback,
    ///Add a breakpoint.
    AddBreakpoint,
    /// Remove a breakpoint.
    RemoveBreakpoint,
    /// Focus the given class in the editor.
    EditorGotoLine,
    /// Add a line to the log.
    AddLineToLog,
    /// Clear the call stack.
    EditorLoadClass,
    /// Add a class to the call stack.
    CallStackClear,
    /// Record the object name for the current object.
    CallStackAdd,
    /// Unused.
    DebugWindowState,
    /// Clear the watch list.
    ClearAWatch,
    /// Add a watch to the watch list.
    AddAWatch,
    /// Lock the watch list.
    LockList,
    /// Unlock the watch list.
    UnlockList,
    /// Set the current object name.
    SetCurrentObjectName,
    /// VA interface End
    GameEnded,
}

impl TryFrom<i32> for VACMD {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(VACMD::ShowDllForm),
            1 => Ok(VACMD::EditorCommand),
            2 => Ok(VACMD::EditorLoadTextBuffer),
            3 => Ok(VACMD::AddClassToHierarchy),
            4 => Ok(VACMD::ClearHierarchy),
            5 => Ok(VACMD::BuildHierarchy),
            6 => Ok(VACMD::ClearWatch),
            7 => Ok(VACMD::AddWatch),
            8 => Ok(VACMD::SetCallback),
            9 => Ok(VACMD::AddBreakpoint),
            10 => Ok(VACMD::RemoveBreakpoint),
            11 => Ok(VACMD::EditorGotoLine),
            12 => Ok(VACMD::AddLineToLog),
            13 => Ok(VACMD::EditorLoadClass),
            14 => Ok(VACMD::CallStackClear),
            15 => Ok(VACMD::CallStackAdd),
            16 => Ok(VACMD::DebugWindowState),
            17 => Ok(VACMD::ClearAWatch),
            18 => Ok(VACMD::AddAWatch),
            19 => Ok(VACMD::LockList),
            20 => Ok(VACMD::UnlockList),
            21 => Ok(VACMD::SetCurrentObjectName),
            22 => Ok(VACMD::GameEnded),
            _ => Err(()),
        }
    }
}

/// Called once from Unreal when the debugger interface is initialized, passing the callback
/// function to use.
///
/// This is the primary entry point into the debugger interface and we use this to
/// launch the effective 'main'.
#[no_mangle]
pub extern "C" fn SetCallback(callback: Option<UnrealCallback>) {
    let cb = callback.expect("Unreal should never give us a null callback.");

    initialize(cb);
}

/// Called each time the debugger breaks, as well as just after SetCallback when the debugger is
/// first initialized.
///
/// Since this implementation doesn't have a UI in-process this does nothing.
#[no_mangle]
pub extern "C" fn ShowDllForm() {
    log::trace!("ShowDllForm");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.show_dll_form();
}

/// Add the given class to the class hierarchy.
///
/// Tells the debugger the names of all currently loaded classes.
#[no_mangle]
pub extern "C" fn AddClassToHierarchy(class_name: *const c_char) {
    log::trace!("AddClassToHierarchy");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.add_class_to_hierarchy(class_name);
}

/// Clear the class hierarchy in the debugger state.
#[no_mangle]
pub extern "C" fn ClearClassHierarchy() {
    log::trace!("ClearClassHierarchy");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.clear_class_hierarchy();
}

/// ???
#[no_mangle]
pub extern "C" fn BuildClassHierarchy() {
    log::trace!("BuildClassHierarchy");
}

/// Legacy version of ClearAWatch.
#[no_mangle]
pub extern "C" fn ClearWatch(kind: i32) {
    log::trace!("ClearWatch {kind}");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.clear_watch(
        WatchKind::from_int(kind).expect("Unreal should never give us a bad watch kind."),
    );
}

/// Removes all watches of the given kind.
///
/// Used when rebuilding the watch list.
/// This occurs each time the debugger breaks to refresh watches.
#[no_mangle]
pub extern "C" fn ClearAWatch(kind: i32) {
    log::trace!("ClearAWatch {kind}");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.clear_watch(
        WatchKind::from_int(kind).expect("Unreal should never give us a bad watch kind."),
    );
}

/// Adds a watch to the watch list for the given kind.
///
/// This is the only Unreal
/// debugger API that returns a value.
#[no_mangle]
pub extern "C" fn AddAWatch(
    kind: i32,
    parent: i32,
    name: *const c_char,
    value: *const c_char,
) -> i32 {
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.add_watch(
        WatchKind::from_int(kind).expect("Unreal should never give us a bad watch kind."),
        parent,
        name,
        value,
    )
}

/// Locks the given watch list.
///
/// Called before Unreal updates the watchlist of the given kind. This will be
/// followed by some number of 'AddAWatch' calls, followed by 'UnlockList'.
#[no_mangle]
pub extern "C" fn LockList(_kind: i32) {
    log::trace!("LockList {_kind}");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.lock_watchlist()
}

/// Unlocks the given watch list.
///
/// Called after Unreal has finished updating the watchlist of the given kind.
#[no_mangle]
pub extern "C" fn UnlockList(kind: i32) {
    log::trace!("UnlockList {kind}");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.unlock_watchlist(
        WatchKind::from_int(kind).expect("Unreal should never give us a bad watch kind."),
    );
}

/// Adds a breakpoint.
///
/// Called in response to an 'addbreakpoint' command.
#[no_mangle]
pub extern "C" fn AddBreakpoint(class_name: *const c_char, line: i32) {
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.add_breakpoint(class_name, line);
}

/// Remove a breakpoint.
///
/// Called in response to a 'removebreakpoint' command.
#[no_mangle]
pub extern "C" fn RemoveBreakpoint(class_name: *const c_char, line: i32) {
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.remove_breakpoint(class_name, line);
}

/// Focus the given class name in the editor.
///
/// For our purposes this API is not necessary. This gets send on a break
/// and any changestack command to indicate what source file to show, but
/// the full filenames of each stack frame are also sent in the CallStackAdd
/// command, and we use that information instead. When switching frames
/// we already know the filename for the frame we switched to.
#[no_mangle]
pub extern "C" fn EditorLoadClass(_class_name: *const c_char) {}

/// Jump to the given line in the editor.
#[no_mangle]
pub extern "C" fn EditorGotoLine(line: i32, _highlight: i32) {
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.goto_line(line);
}

/// A line has been added to the log.
#[no_mangle]
pub extern "C" fn AddLineToLog(text: *const c_char) {
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.add_line_to_log(text);
}

/// Clear the call stack.
///
/// This is called after Unreal breaks, and is followed by one or more calls
/// to 'CallstackAdd'.
#[no_mangle]
pub extern "C" fn CallStackClear() {
    log::trace!("CallStackClear");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.clear_callstack();
}

/// Add the given class name to the call stack. Call stacks are built bottom-up
/// from the deepest call in the stack to the top-most call.
#[no_mangle]
pub extern "C" fn CallStackAdd(class_name: *const c_char) {
    log::trace!("CallStackAdd");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.add_frame(class_name);
}

/// Record the object name for the current object (this).
#[no_mangle]
pub extern "C" fn SetCurrentObjectName(obj_name: *const c_char) {
    log::trace!("SetCurrentObjectName");
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.current_object_name(obj_name);
}

/// Unused.
#[no_mangle]
pub extern "C" fn DebugWindowState(code: i32) {
    log::trace!("DebugWindowState {code}");
}

/// VA interface
#[no_mangle]
pub extern "C" fn IPCSetCallbackUC(callback: Option<UnrealVADebugCallback>) {
    let cb = callback.expect("Unreal should never give us a null callback.");
    va_initialized(cb);

    // send_command_by_va_callback(b"break");
}

/// VA interface Unreal tick notification.
#[no_mangle]
pub extern "C" fn IPCNotifyBeginTick() {
    if !game_runtime_is_initialized() {
        init_game_runtime();
    }
    set_game_runtime_in_break(false);
    if let Some(rt) = get_game_runtime_option_mut().as_mut() {
        rt.tick();
    }
    consume_game_runtime_pending_commands();
}

/// VA interface
#[no_mangle]
pub extern "C" fn IPCNotifyDebugInfo(_param: u32) -> u32 {
    1
}

/// VA interface
#[no_mangle]
pub extern "C" fn IPCnFringeSupport(version: i32) {
    log::trace!("IPCnFringeSupport {version}");
}

/// VA interface
#[no_mangle]
pub extern "C" fn IPCSendCommandToVS(
    cmd_id: i32,
    dw_1: DWORD,
    dw_2: DWORD,
    s_1: LPCWSTR,
    s_2: LPCWSTR,
) -> i32 {
    let Ok(cmd) = VACMD::try_from(cmd_id) else {
        log::error!("Unknown command id: {cmd_id}");
        return -1;
    };
    let mut hnd = DEBUGGER.lock().unwrap();
    let dbg = hnd.as_mut().unwrap();
    dbg.ipc_send_command_to_vs(cmd, dw_1, dw_2, s_1, s_2)
}
