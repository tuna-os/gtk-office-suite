//! `collabd [--data-dir DIR]`: Commands on stdin, Events on stdout, logs on stderr.
//!
//! The data dir (default `$COLLABD_DATA_DIR`, else `$XDG_DATA_HOME/tuna-collabd`,
//! else `~/.local/share/tuna-collabd`) holds the Streams identity key and nothing
//! else; see `collabd::identity` for exactly what.

use std::path::PathBuf;
use std::sync::Arc;

use collabd::connector::DefaultConnector;
use collabd::daemon::{Daemon, Input};
use collabd::output::{self, Output};
use collabd::log;
use suite_collab::LineReader;

fn data_dir(args: &[String]) -> Result<PathBuf, String> {
    match args {
        [] => {}
        [flag, dir] if flag == "--data-dir" => return Ok(PathBuf::from(dir)),
        _ => return Err(format!("unexpected arguments {args:?}; usage: collabd [--data-dir DIR]")),
    }
    if let Some(d) = std::env::var_os("COLLABD_DATA_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(d).join("tuna-collabd"));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".local/share/tuna-collabd"))
        .ok_or_else(|| "no data dir: set --data-dir, COLLABD_DATA_DIR or HOME".to_owned())
}

/// Read stdin on its own thread (a blocking read must not sit on the runtime) and
/// turn each line into an [`Input`].
fn read_stdin(tx: tokio::sync::mpsc::UnboundedSender<Input>) {
    let stdin = std::io::stdin();
    let mut lines = LineReader::new(stdin.lock());
    loop {
        let input = match lines.read_line() {
            Ok(line) => match Input::from_line(line) {
                Some(i) => i,
                None => return,
            },
            Err(e) => {
                log!("stdin: {e}");
                return;
            }
        };
        if tx.send(input).is_err() {
            return;
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = match data_dir(&args) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("collabd: {e}");
            std::process::exit(2);
        }
    };
    let device = format!(
        "collabd-{}",
        hex::encode(dcp_streams::crypto::random::<6>())
    );
    log!("starting, device {device}, data dir {}", dir.display());

    let (out, rx) = Output::channel();
    let writer = tokio::task::spawn_blocking(move || {
        let mut stdout = std::io::stdout().lock();
        output::write_all(rx, &mut stdout)
    });

    let (tx, input) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || read_stdin(tx));

    let daemon = Daemon::new(out, Arc::new(DefaultConnector::new(dir)), device);
    daemon.run(input).await;
    // `run` has dropped every Output: the writer drains what is left and ends.
    match writer.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            eprintln!("collabd: stdout: {e}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("collabd: writer task: {e}");
            std::process::exit(1);
        }
    }
}
