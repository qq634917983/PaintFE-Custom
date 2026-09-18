//! Step-by-step control of a running PaintFE GUI from the command line.
//!
//! The GUI owns all application state. A small localhost TCP server forwards one
//! textual command at a time to the GUI thread and waits for its response.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

const DEFAULT_ADDRESS: &str = "127.0.0.1:47831";

pub const HELP: &str = "Commands:
  status                         show current UI/document state
  focus                          focus the PaintFE window
  open <path>                    open an image/project in the GUI
  new <width> <height>           create a new document
  tool <name>                    select a tool (brush, eraser, pencil, ...)
  color <#RRGGBB[AA]>            set the primary color
  secondary-color <#RRGGBB[AA]>  set the secondary color
  brush-size <pixels>            set brush/tool size
  zoom <percent|fit>             set zoom or fit canvas to the window
  undo | redo                    move one history step
  save                           invoke the normal GUI save action
  wait <milliseconds>            pause the interactive client
  help                           show this help
  exit                           leave the interactive client";

pub struct UiControlRequest {
    pub command: String,
    pub response: mpsc::Sender<String>,
}

pub fn is_client_mode() -> bool {
    matches!(
        std::env::args().nth(1).as_deref(),
        Some("ui") | Some("--ui-control")
    )
}

fn address() -> String {
    std::env::var("PAINTFE_UI_ADDRESS").unwrap_or_else(|_| DEFAULT_ADDRESS.to_owned())
}

/// Start the GUI-side command listener. Failure (usually an occupied port) is
/// reported in the log and leaves the app otherwise fully usable.
pub fn start_server(ctx: eframe::egui::Context) -> mpsc::Receiver<UiControlRequest> {
    let (request_tx, request_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let addr = address();
        let listener = match TcpListener::bind(&addr) {
            Ok(listener) => listener,
            Err(error) => {
                crate::logger::write(
                    "WARN",
                    &format!("UI control unavailable at {addr}: {error}"),
                );
                return;
            }
        };
        crate::logger::write("INFO", &format!("UI control listening at {addr}"));

        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(7)));
            let mut command = String::new();
            if BufReader::new(&stream).read_line(&mut command).is_err() {
                continue;
            }
            let command = command.trim().to_owned();
            let (response_tx, response_rx) = mpsc::channel();
            if request_tx
                .send(UiControlRequest {
                    command,
                    response: response_tx,
                })
                .is_err()
            {
                break;
            }
            ctx.request_repaint();
            let response = response_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| "ERR GUI did not respond in time".to_owned());
            let _ = writeln!(stream, "{response}");
        }
    });
    request_rx
}

fn send(command: &str) -> Result<String, String> {
    let addr = address();
    let mut stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|_| format!("invalid PAINTFE_UI_ADDRESS: {addr}"))?,
        Duration::from_secs(2),
    )
    .map_err(|error| {
        format!("cannot connect to PaintFE at {addr}: {error}\nStart the PaintFE GUI first.")
    })?;
    stream
        .write_all(format!("{}\n", command.trim()).as_bytes())
        .map_err(|error| error.to_string())?;
    let _ = stream.shutdown(Shutdown::Write);
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| error.to_string())?;
    Ok(response.trim_end().to_owned())
}

/// Run either a one-shot command (`PaintFE ui status`) or an interactive REPL
/// (`PaintFE ui`) in which every entered line advances the GUI by one action.
pub fn run_client() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if !args.is_empty() {
        return match send(&args.join(" ")) {
            Ok(response) => {
                println!("{response}");
                if response.starts_with("ERR ") { 1 } else { 0 }
            }
            Err(error) => {
                eprintln!("error: {error}");
                1
            }
        };
    }

    println!("PaintFE step UI controller ({})", address());
    println!("Type 'help' for commands; each line executes one GUI step.");
    let stdin = io::stdin();
    loop {
        print!("paintfe-ui> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let command = line.trim();
        if command.is_empty() {
            continue;
        }
        if matches!(command, "exit" | "quit") {
            break;
        }
        if command == "help" {
            println!("{HELP}");
            continue;
        }
        if let Some(ms) = command.strip_prefix("wait ") {
            match ms.trim().parse::<u64>() {
                Ok(ms) => {
                    std::thread::sleep(Duration::from_millis(ms));
                    println!("OK waited {ms}ms");
                }
                Err(_) => eprintln!("ERR wait expects milliseconds"),
            }
            continue;
        }
        match send(command) {
            Ok(response) => println!("{response}"),
            Err(error) => eprintln!("error: {error}"),
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_documents_step_commands() {
        for command in ["status", "open", "new", "tool", "color", "undo", "redo"] {
            assert!(HELP.contains(command));
        }
    }
}
