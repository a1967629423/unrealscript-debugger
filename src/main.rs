use std::io::BufReader;

use adapter::UnrealscriptAdapter;
use dap::prelude::*;
use flexi_logger::{FileSpec, Logger};

pub mod adapter;
pub mod client;

use client::UnrealscriptClient;

fn main() {
    let _logger = Logger::try_with_env_or_str("trace")
        .unwrap()
        .log_to_file(FileSpec::default().directory(
            "C:\\users\\jonat\\projects\\debugger\\unrealscript-debugger-interface\\logs",
        ))
        .start()
        .unwrap();

    let adapter = UnrealscriptAdapter {};
    let client = UnrealscriptClient::new(std::io::stdout());
    let mut server = Server::new(adapter, client);

    let _event_sender = server.clone_sender();
    log::info!("Ready to start!");

    // Spawn a new thread for the server to process messages in. This will loop
    // until the debugger quits.
    let server_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(std::io::stdin());
        server.run(&mut reader)
    });

    // Wait for the server to finish processing. We may not necessarily get back here at
    // all: the client may kill the adapter process if it hits certain errors.
    match server_thread.join().unwrap() {
        Ok(()) => std::process::exit(0),
        Err(err) => {
            log::error!("Debugger failed with error {err}");
            eprintln!("Debugger failed with error {err:#?}");
            std::process::exit(1);
        }
    };
}
