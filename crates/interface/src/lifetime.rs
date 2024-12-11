//! Lifetime management for the Unrealscript debugger interface.
//!
//! This module contains functions and data structures related to maintaining
//! state outside of Unreal for the lifetime of a debugging session.
//!
//! The debug adapter DLL's lifetime is entirely controlled by Unreal. The
//! DLL is loaded when a debugging session starts, and is unloaded when the
//! debugging session ends. This is a total unload and the DLL is unmapped from
//! memory entirely, so we need to be careful to allow for graceful shutdown
//! when the debugging session is ending. This is made more difficult because
//! Unreal doesn't actually directly tell us when this is going to happen,
//! we can only infer it.
//!
//! A debugging session can start in two ways:
//!
//! - When the game is launched with -autoDebug. The debugger interface is then
//! loaded as part of game startup and we receive a normal init sequence and then
//! unreal will automatically break at the first opportunity.
//!
//! - When the user enters a '\toggledebugger' command. The debugger interface
//! is loaded and we receive an init sequence, but Unreal does not break.
//!
//! The debugging session can end in three ways:
//!
//! - When the user quits the game while a debugging session is active.
//! - When the user enters the '\toggledebugger' command while a debugging
//! session is active.
//! - When the 'stopdebugging' command is sent to Unreal.
//!
//! The first two cases are the same, the last is slightly different because
//! the command originates inside the debugger interface. But all three perform
//! the same shutdown sequence and then unload the DLL from memory. We must be
//! _very_ careful to ensure this happens cleanly, as any lingering code that
//! tries to run after the DLL is unloaded will almost certainly cause a crash.
//!
//! The 'initialize' function is used to set up the debugger state when we are
//! starting a debugging session.

use std::{net::SocketAddr, thread};

use common::{
    create_logger, UnrealCommand, UnrealInterfaceMessage, DEFAULT_PORT, DEFAULT_PORT_TRY_NUM,
    PORT_TRY_NUM_VAR, PORT_VAR,
};
use futures::channel::mpsc::{self, unbounded, UnboundedReceiver};
use futures::prelude::*;
use futures::select;
use tokio::net::{TcpListener, TcpStream};
use tokio_serde::formats::SymmetricalJson;
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

use crate::{get_game_runtime_mut, is_game_runtime_in_break};
use crate::{
    api::{UnrealCallback, UnrealVADebugCallback},
    debugger::{CommandAction, Debugger, DebuggerError}, DEBUGGER, LOGGER, VARIABLE_REQUST_CONDVAR,
};
use async_compat::{Compat,CompatExt};

/// Initialize the debugger instance. This should be called exactly once when
/// Unreal first initializes us. Responsible for building the shared state object
/// the other Unreal entry points will use and spawning the main loop thread
/// that will perform I/O with the debugger adapter.
pub fn initialize(cb: UnrealCallback) {
    if let Ok(dbg) = DEBUGGER.lock().as_mut() {
        assert!(dbg.is_none(), "Initialize already called.");

        // Start the logger. If this fails there isn't much we can do.
        init_logger();

        // Register a panic handler that will log to the log file, since our stdout/stderr
        // are not connected to anything.
        std::panic::set_hook(Box::new(|p| {
            log::error!("Panic: {p:#?}");
        }));

        // Create a channel pair for shutting down the interface. This is used when
        // we receive a signal that Unreal is about to kill the debugging session. The
        // debugger instance owns the tx side and can send the event when this happens.
        // The separate thread we spawn below owns the receiving side and uses this to
        // cleanly stop itself.
        let (ctx, crx) = unbounded();

        // Start the main loop that will listen for connections so we can
        // communicate the debugger state to the adapter. This will spin up a
        // new async runtime for this thread only and wait for the main loop
        // to complete.
        let handle = thread::spawn(move || {
            // let rt = Builder::new_current_thread()
            //     .enable_io()
            //     .build()
            //     .expect("Failed to create runtime");
            // rt.block_on(async {
            //     match main_loop(cb, crx).await {
            //         Ok(()) => (),
            //         Err(e) => {
            //             // Something catastrophic failed in the main loop. Log it
            //             // and exit the thread, there is little else we can do.
            //             log::error!("Error in debugger main loop: {e}");
            //         }
            //     }
            // });
            // init_runtime();
            // get_runtime_mut().spawn();
            futures::executor::block_on(Compat::new(async move {
                match main_loop(cb, crx).await {
                    Ok(()) => (),
                    Err(e) => {
                        // Something catastrophic failed in the main loop. Log it
                        // and exit the thread, there is little else we can do.
                        log::error!("Error in debugger main loop: {e}");
                    }
                }
            }));

            // loop {
            //     get_runtime_mut().tick();
            //     std::thread::sleep(Duration::from_millis(100));
            // }
        });

        // Construct the debugger state.
        dbg.replace(Debugger::new(ctx, Some(handle)));
    }
}
/// TODO
pub fn va_initialized(cb: UnrealVADebugCallback) {
    if let Ok(dbg) = DEBUGGER.lock().as_mut() {
        assert!(dbg.is_none(), "Initialize already called.");

        // Start the logger. If this fails there isn't much we can do.
        init_logger();

        // Register a panic handler that will log to the log file, since our stdout/stderr
        // are not connected to anything.
        std::panic::set_hook(Box::new(|p| {
            log::error!("Panic: {p:#?}");
        }));

        // Create a channel pair for shutting down the interface. This is used when
        // we receive a signal that Unreal is about to kill the debugging session. The
        // debugger instance owns the tx side and can send the event when this happens.
        // The separate thread we spawn below owns the receiving side and uses this to
        // cleanly stop itself.
        let (ctx, crx) = unbounded();

        let handle = thread::spawn(move || {
            // let rt = Builder::new_current_thread()
            //     .enable_io()
            //     .build()
            //     .expect("Failed to create runtime");
            // rt.block_on(async {
            //     match main_loop(cb, crx).await {
            //         Ok(()) => (),
            //         Err(e) => {
            //             // Something catastrophic failed in the main loop. Log it
            //             // and exit the thread, there is little else we can do.
            //             log::error!("Error in debugger main loop: {e}");
            //         }
            //     }
            // });
            // init_runtime();
            // get_runtime_mut().spawn(Compat::new(async move {
            //     match va_main_loop(cb, crx).await {
            //         Ok(()) => (),
            //         Err(e) => {
            //             // Something catastrophic failed in the main loop. Log it
            //             // and exit the thread, there is little else we can do.
            //             log::error!("Error in debugger main loop: {e}");
            //         }
            //     }
            // }));
            futures::executor::block_on(Compat::new(async move {
                match va_main_loop(cb, crx).await {
                    Ok(()) => (),
                    Err(e) => {
                        // Something catastrophic failed in the main loop. Log it
                        // and exit the thread, there is little else we can do.
                        log::error!("Error in debugger main loop: {e}");
                    }
                }
            }));

            // loop {
            //     get_runtime_mut().tick();
            //     std::thread::sleep(Duration::from_millis(100));
            // }
        });

        // Construct the debugger state.
        let mut new_dbg = Debugger::new(ctx, Some(handle));
        new_dbg.set_saw_show_dll(true);
        dbg.replace(new_dbg);
    }
}

/// Initialize the logging interface.
fn init_logger() {
    let mut logger = LOGGER.lock().unwrap();
    assert!(logger.is_none(), "Already have a logger. Multiple inits?");
    let new_logger = create_logger("interface");
    logger.replace(new_logger);
}

/// An enum representing the result of a client connection.
enum ConnectionResult {
    /// The client disconnected without signalling that the debugging session
    /// should end. This could be due to an interruption and the client may
    /// be able to reconnect later. This result means the lifetime of the debugging
    /// session hasn't ended and that we should try to accept another connection.
    Disconnected,
    Shutdown,
}

// Determine the port number to use. If the environment has a valid port number
// use that, otherwise use the default port.
fn determine_port() -> u16 {
    if let Ok(str) = std::env::var(PORT_VAR) {
        match str.parse::<u16>() {
            Ok(v) => {
                return v;
            }
            Err(_) => {
                log::error!("Bad port value in {}: {str}", PORT_VAR);
            }
        }
    }

    DEFAULT_PORT
}

/// Determine the number of times to try to bind to a port before giving up.
fn determine_try_num() -> u16 {
    if let Ok(str) = std::env::var(PORT_TRY_NUM_VAR) {
        match str.parse::<u16>() {
            Ok(v) => {
                return v;
            }
            Err(_) => {
                log::error!("Bad try_num value in TRY_NUM: {str}");
            }
        }
    }

    DEFAULT_PORT_TRY_NUM
}

/// Create a TPC connection. If the connection is already occupied, try the next port until it reaches try_num times and return an error.
/// For other errors, return directly
async fn create_tcp_listener(
    mut addr: SocketAddr,
    base_port: u16,
    mut try_num: u16,
) -> tokio::io::Result<TcpListener> {
    let mut port = base_port;
    addr.set_port(port);
    while try_num > 0 {
        match TcpListener::bind(addr).compat().await {
            Ok(listener) => {
                return Ok(listener);
            }
            Err(e) => {
                if !matches!(e.kind(), std::io::ErrorKind::AddrInUse) {
                    log::error!("Failed to bind to port {port}: {e}");
                    return Err(e);
                }
            }
        }
        try_num -= 1;
        port += 1;
        addr.set_port(port);
    }

    return Err(tokio::io::Error::new(
        tokio::io::ErrorKind::AddrInUse,
        "Failed to bind to port",
    ));
}

/// The main worker thread for the debugger interface. This is created when the
/// debugger session is created, and returns when the debugger session ends.
async fn main_loop(
    cb: UnrealCallback,
    mut crx: UnboundedReceiver<()>,
) -> Result<(), tokio::io::Error> {
    let port = determine_port();

    log::info!("Listening for connections on port {port}");
    // Start listening on a socket for connections from the adapter.
    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("Failed to parse address");

    let server = create_tcp_listener(addr, port, determine_try_num()).await?;
    
    log::trace!("create server success");

    loop {
        select! {
            conn = server.accept().compat().fuse() => {
                let (mut socket, addr) = conn?;
                log::info!("Received connection from {addr}");
                match handle_connection(&mut socket, AutoDebugSendToUnreal::new(cb), &mut crx).await? {
                    // Client disconnected: keep looping and accept another connection
                    ConnectionResult::Disconnected => (),
                    // We're shutting down: close down this loop.
                    ConnectionResult::Shutdown => break,
                }
            }
            _ = crx.next() => {
                log::info!("Received shutdown message. Closing main loop.");
                break;
            }
        }
    }
    Ok(())
}

async fn va_main_loop(cb:UnrealVADebugCallback,mut crx: UnboundedReceiver<()>) -> Result<(),std::io::Error> {
    let port = determine_port();

    log::info!("Listening for connections on port {port}");
    // Start listening on a socket for connections from the adapter.
    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .expect("Failed to parse address");

    let server = create_tcp_listener(addr, port, determine_try_num()).await?;

    loop {
        select! {
            conn = server.accept().compat().fuse() => {
                let (mut socket, addr) = conn?;
                log::info!("Received connection from {addr}");
                match handle_connection(&mut socket, VaDebugSendToUnreal::new(cb), &mut crx).await? {
                    // Client disconnected: keep looping and accept another connection
                    ConnectionResult::Disconnected => (),
                    // We're shutting down: close down this loop.
                    ConnectionResult::Shutdown => break,
                }
            }
            _ = crx.next() => {
                log::info!("Received shutdown message. Closing main loop.");
                break;
            }
        }
    }
    Ok(())
}

/// TODO:
pub trait SendToUnreal {
    /// TODO:
    fn send_bytes(&self, bytes: &[u8]);
}

struct AutoDebugSendToUnreal {
    callback: UnrealCallback,
}

impl AutoDebugSendToUnreal {
    pub fn new(callback: UnrealCallback) -> Self {
        Self { callback }
    }
}

impl SendToUnreal for AutoDebugSendToUnreal {
    fn send_bytes(&self, bytes: &[u8]) {
        (self.callback)(bytes.as_ptr());
    }
}

struct VaDebugSendToUnreal {
    callback: UnrealVADebugCallback,
}
impl VaDebugSendToUnreal {
    pub fn new(callback: UnrealVADebugCallback) -> Self {
        Self { callback }
    }
}

impl SendToUnreal for VaDebugSendToUnreal {
    fn send_bytes(&self, bytes: &[u8]) {
        let str = String::from_utf8_lossy(&bytes[..bytes.len() - 1]).to_string();
        let game_callback = self.callback;
        if !is_game_runtime_in_break() {
            get_game_runtime_mut().spawn(async move {
                let mut bytes = str.encode_utf16().collect::<Vec<_>>();
                bytes.push(0);
                (game_callback)(0,bytes.as_ptr());
            });
            return;
        }
        let mut bytes = str.encode_utf16().collect::<Vec<_>>();
        bytes.push(0);
        (game_callback)(0,bytes.as_ptr());
    }
}

/// Accept one connection from the debugger adapter and process commands from it until it
/// disconnects.
///
/// We accept only a single connection at a time, if multiple adapters attempt to connect
/// we'll process them in sequence.
async fn handle_connection<T:SendToUnreal>(
    stream: &mut TcpStream,
    cb: T,
    crx: &mut UnboundedReceiver<()>,
) -> Result<ConnectionResult, tokio::io::Error> {
    // Create a new message passing channel and send the sender to the debugger.
    // It's convenient to have a per-connection message channel as it also serves
    // as an indicator within the debugger to tell if the interface is connected.
    let (etx, mut erx) = mpsc::unbounded();

    {
        let mut hnd = DEBUGGER.lock().unwrap();
        let dbg = hnd.as_mut().unwrap();
        dbg.new_connection(etx);
    }

    let (reader, writer) = stream.split();
    let delimiter = FramedRead::new(reader, LengthDelimitedCodec::new());

    let mut deserializer = tokio_serde::SymmetricallyFramed::new(
        delimiter,
        SymmetricalJson::<UnrealCommand>::default(),
    );

    let delimiter = FramedWrite::new(writer, LengthDelimitedCodec::new());
    let mut serializer = tokio_serde::SymmetricallyFramed::new(
        delimiter,
        SymmetricalJson::<UnrealInterfaceMessage>::default(),
    );

    loop {
        select! {
            command = deserializer.try_next().compat().fuse() => {
                match command? {
                    Some(command) => {
                        match dispatch_command(command) {
                            CommandAction::Nothing => (),
                            CommandAction::Callback(vec) => cb.send_bytes(&vec),
                            CommandAction::MultiStepCallback(vec) => {
                                for v in vec {
                                    cb.send_bytes(&v);
                                }
                            }
                        }
                    },
                    None => break,
                };
            },
            evt = erx.next() => {
                match evt {
                    Some(evt) => if let Err(e) = serializer.send(evt).compat().await {
                        // If we fail to send the packet then the connection has been
                        // interrupted and we can return.
                        log::error!("Error sending event to adapter: {e}");
                        break
                    },
                    None => break,
                };
            },
            _ = crx.next() => {
                log::info!("Received shutdown message. Closing connection.");
                return Ok(ConnectionResult::Shutdown);
            }
        }
    }

    log::info!("Client disconnected.");
    Ok(ConnectionResult::Disconnected)
}

fn dispatch_command(command: UnrealCommand) -> CommandAction {
    let mut hnd = DEBUGGER.lock().unwrap();
    loop {
        let dbg = hnd.as_mut().unwrap();
        if dbg.pending_variable_request() {
            // There is still an outstanding variable request. We can't do anything until
            // this is finished.
            log::info!("Waiting for variable request to complete...");
            hnd = VARIABLE_REQUST_CONDVAR.wait(hnd).unwrap();
        } else {
            break;
        }
    }
    let dbg = hnd.as_mut().unwrap();
    match dbg.handle_command(command) {
        Ok(action) => action,

        Err(DebuggerError::NotConnected) => {
            log::error!("Not connected");
            CommandAction::Nothing
        }
    }
}