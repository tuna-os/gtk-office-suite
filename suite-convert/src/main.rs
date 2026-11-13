// suite-convert — the command line (ADR 0015).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::process::ExitCode;

use suite_convert::{engine, parse, plan, Command, Exit, HELP};

fn main() -> ExitCode {
    let command = match parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("suite-convert: {error}\nTry `suite-convert --help`.");
            return ExitCode::from(Exit::Usage.code());
        }
    };
    match command {
        Command::Help => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("suite-convert {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Convert(args) => {
            let plan = match plan(&args) {
                Ok(plan) => plan,
                Err(error) => {
                    eprintln!("suite-convert: {error}");
                    return ExitCode::from(Exit::Usage.code());
                }
            };
            match engine::convert(&args, &plan) {
                Ok(outcome) => {
                    if args.report_loss {
                        // JSON to stderr so `-o -` keeps stdout for the document.
                        eprintln!("{}", outcome.report_json(&plan));
                    }
                    match args.fail_on_loss {
                        Some(budget) if outcome.over(budget) => {
                            eprintln!("suite-convert: the conversion lost {} content, over the --fail-on-loss budget", outcome.worst().name());
                            ExitCode::from(Exit::Loss.code())
                        }
                        _ => ExitCode::SUCCESS,
                    }
                }
                Err(failure) => {
                    eprintln!("suite-convert: {}", failure.message);
                    ExitCode::from(failure.exit.code())
                }
            }
        }
    }
}
