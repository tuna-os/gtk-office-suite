// suite-convert — the command line (ADR 0015). Phase 1: arguments and routing,
// no engines yet, so a valid request exits 3 and says so.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::process::ExitCode;

use suite_convert::{parse, plan, Command, Exit, HELP};

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
        Command::Convert(args) => match plan(&args) {
            Ok(plan) => {
                eprintln!(
                    "suite-convert: {} to {} is a supported conversion, but no engine is wired in yet (phase 2 of HEADLESS-CONVERSION-SPEC).",
                    plan.from.name(),
                    plan.to.name()
                );
                ExitCode::from(Exit::Convert.code())
            }
            Err(error) => {
                eprintln!("suite-convert: {error}");
                ExitCode::from(Exit::Usage.code())
            }
        },
    }
}
