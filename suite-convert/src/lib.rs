// suite-convert — headless document conversion for the suite (ADR 0015).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Phase 1 of HEADLESS-CONVERSION-SPEC: the command line, the formats and what
// converts to what, and the exit codes. No conversion happens here yet.

pub mod engine;

use std::fmt;
use std::path::{Path, PathBuf};

/// Exit codes. An API from the first release: a code is added, never
/// renumbered (ADR 0015, decision 5).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Exit {
    /// Converted, within any loss budget.
    Ok = 0,
    /// Bad arguments, or the input does not exist.
    Usage = 1,
    /// The input could not be read as the format it claims.
    Parse = 2,
    /// The conversion or writing the output failed.
    Convert = 3,
    /// Converted, but the loss exceeds `--fail-on-loss`.
    Loss = 4,
}

impl Exit {
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A document format the tool names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Format {
    Md,
    Odt,
    Docx,
    Html,
    Pdf,
    Csv,
    Ods,
    Xlsx,
    Odp,
    Pptx,
    Png,
}

/// Which family of document a format belongs to: it decides what converts to what.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Category {
    Text,
    Spreadsheet,
    Presentation,
}

impl Format {
    /// The format a file extension names (case-insensitive, no dot).
    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Self::Md,
            "odt" => Self::Odt,
            "docx" => Self::Docx,
            "html" | "htm" => Self::Html,
            "pdf" => Self::Pdf,
            "csv" => Self::Csv,
            "ods" => Self::Ods,
            "xlsx" => Self::Xlsx,
            "odp" => Self::Odp,
            "pptx" => Self::Pptx,
            "png" => Self::Png,
            _ => return None,
        })
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Md => "md",
            Self::Odt => "odt",
            Self::Docx => "docx",
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Csv => "csv",
            Self::Ods => "ods",
            Self::Xlsx => "xlsx",
            Self::Odp => "odp",
            Self::Pptx => "pptx",
            Self::Png => "png",
        }
    }

    /// The family this format belongs to. PDF and PNG are outputs of every
    /// family, so they have none of their own.
    pub fn category(self) -> Option<Category> {
        match self {
            Self::Md | Self::Odt | Self::Docx | Self::Html => Some(Category::Text),
            Self::Csv | Self::Ods | Self::Xlsx => Some(Category::Spreadsheet),
            Self::Odp | Self::Pptx => Some(Category::Presentation),
            Self::Pdf | Self::Png => None,
        }
    }
}

/// Whether `from` converts to `to`, from the table in HEADLESS-CONVERSION-SPEC.
pub fn supported(from: Format, to: Format) -> bool {
    use Format::*;
    let Some(category) = from.category() else {
        return false;
    };
    match category {
        Category::Text => {
            matches!(from, Md | Odt | Docx) && matches!(to, Odt | Docx | Md | Html | Pdf | Png)
        }
        Category::Spreadsheet => matches!(to, Ods | Xlsx | Csv | Html | Pdf | Png),
        Category::Presentation => matches!(to, Odp | Pptx | Pdf | Png),
    }
}

/// How much was lost, in order: `None < Minor < Major < Critical`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LossLevel {
    None,
    Minor,
    Major,
    Critical,
}

impl LossLevel {
    pub fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "minor" => Self::Minor,
            "major" => Self::Major,
            "critical" => Self::Critical,
            _ => return None,
        })
    }
}

/// Where the input comes from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Input {
    Path(PathBuf),
    Stdin,
    /// `--batch GLOB`: every file the quoted glob matches.
    Batch(String),
}

/// Where the output goes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Output {
    Path(PathBuf),
    Stdout,
}

/// The command line, read and checked for shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Args {
    pub input: Input,
    pub output: Option<Output>,
    pub from: Option<Format>,
    pub to: Option<Format>,
    pub report_loss: bool,
    pub fail_on_loss: Option<LossLevel>,
    pub verbose: bool,
    /// For `--to png`: the page (1-based) and the width in pixels.
    pub page: u32,
    pub size: u32,
}

/// What the command line asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Convert(Args),
    Help,
    Version,
}

/// Why the arguments were refused. All of these exit with [`Exit::Usage`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UsageError {
    NoInput,
    UnknownOption(String),
    MissingValue(String),
    BadValue {
        option: String,
        value: String,
    },
    TooManyArguments(String),
    /// A format could not be worked out and none was given.
    UnknownFormat {
        path: String,
    },
    /// `-` (stdin or stdout) needs the format said outright.
    NeedsFormat(&'static str),
    Unsupported {
        from: Format,
        to: Format,
    },
    /// `--batch` needs an output directory and a target format.
    BatchNeeds(&'static str),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInput => f.write_str("no input file given"),
            Self::UnknownOption(o) => write!(f, "unknown option `{o}`"),
            Self::MissingValue(o) => write!(f, "`{o}` needs a value"),
            Self::BadValue { option, value } => {
                write!(f, "`{value}` is not a valid value for `{option}`")
            }
            Self::TooManyArguments(a) => write!(f, "unexpected argument `{a}`"),
            Self::UnknownFormat { path } => write!(
                f,
                "cannot tell the format of `{path}`; say it with --from or --to"
            ),
            Self::NeedsFormat(which) => write!(f, "`-` needs --{which} to say the format"),
            Self::Unsupported { from, to } => write!(
                f,
                "converting {} to {} is not supported",
                from.name(),
                to.name()
            ),
            Self::BatchNeeds(what) => write!(f, "--batch needs {what}"),
        }
    }
}

impl std::error::Error for UsageError {}

fn value(option: &str, args: &mut impl Iterator<Item = String>) -> Result<String, UsageError> {
    args.next()
        .ok_or_else(|| UsageError::MissingValue(option.to_owned()))
}

fn format_value(option: &str, raw: &str) -> Result<Format, UsageError> {
    Format::from_extension(raw.trim_start_matches('.')).ok_or_else(|| UsageError::BadValue {
        option: option.to_owned(),
        value: raw.to_owned(),
    })
}

fn number(option: &str, raw: &str) -> Result<u32, UsageError> {
    raw.parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| UsageError::BadValue {
            option: option.to_owned(),
            value: raw.to_owned(),
        })
}

/// Read the command line (without the program name).
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, UsageError> {
    let mut args = args.into_iter();
    let mut positional: Vec<String> = Vec::new();
    let (mut output, mut from, mut to, mut batch) = (None, None, None, None);
    let (mut report_loss, mut fail_on_loss, mut verbose) = (false, None, false);
    let (mut page, mut size) = (1, 256);
    let mut only_positional = false;

    while let Some(arg) = args.next() {
        if only_positional || arg == "-" || !arg.starts_with('-') {
            positional.push(arg);
            continue;
        }
        // `--option=value` is the same as `--option value`.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let take = |args: &mut I::IntoIter| match &inline {
            Some(v) => Ok(v.clone()),
            None => value(&name, args),
        };
        match name.as_str() {
            "--" => only_positional = true,
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "-v" | "--verbose" => verbose = true,
            "--report-loss" => report_loss = true,
            "-o" | "--output" => output = Some(take(&mut args)?),
            "-f" | "--from" => from = Some(format_value(&name, &take(&mut args)?)?),
            "-t" | "--to" => to = Some(format_value(&name, &take(&mut args)?)?),
            "--batch" => batch = Some(take(&mut args)?),
            "--fail-on-loss" => {
                let raw = take(&mut args)?;
                fail_on_loss = Some(LossLevel::parse(&raw).ok_or(UsageError::BadValue {
                    option: name.clone(),
                    value: raw,
                })?);
            }
            "--page" => page = number(&name, &take(&mut args)?)?,
            "--size" => size = number(&name, &take(&mut args)?)?,
            _ => return Err(UsageError::UnknownOption(name)),
        }
    }

    let mut positional = positional.into_iter();
    let (input, positional_output) = if let Some(glob) = batch {
        if let Some(extra) = positional.next() {
            return Err(UsageError::TooManyArguments(extra));
        }
        (Input::Batch(glob), None)
    } else {
        let input = positional.next().ok_or(UsageError::NoInput)?;
        let input = if input == "-" {
            Input::Stdin
        } else {
            Input::Path(input.into())
        };
        (input, positional.next())
    };
    if let Some(extra) = positional.next() {
        return Err(UsageError::TooManyArguments(extra));
    }

    // `-o` wins over the positional output; either may be `-` for stdout.
    let output = output.or(positional_output).map(|o| {
        if o == "-" {
            Output::Stdout
        } else {
            Output::Path(o.into())
        }
    });

    Ok(Command::Convert(Args {
        input,
        output,
        from,
        to,
        report_loss,
        fail_on_loss,
        verbose,
        page,
        size,
    }))
}

/// What to convert and to what, once the formats are worked out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub from: Format,
    pub to: Format,
}

/// Work out the formats from the arguments and check the conversion exists.
/// Formats given outright win over the ones the extensions say.
pub fn plan(args: &Args) -> Result<Plan, UsageError> {
    if let Input::Batch(_) = args.input {
        if args.to.is_none() {
            return Err(UsageError::BatchNeeds("a target format (--to)"));
        }
        if !matches!(args.output, Some(Output::Path(_))) {
            return Err(UsageError::BatchNeeds("an output directory (-o DIR)"));
        }
    }

    let from = match (args.from, &args.input) {
        (Some(f), _) => f,
        (None, Input::Path(p)) => {
            Format::from_path(p).ok_or_else(|| UsageError::UnknownFormat {
                path: p.display().to_string(),
            })?
        }
        (None, Input::Stdin) => return Err(UsageError::NeedsFormat("from")),
        // A batch's inputs are named by the glob's extension, when it has one.
        (None, Input::Batch(g)) => Format::from_path(Path::new(g))
            .ok_or_else(|| UsageError::UnknownFormat { path: g.clone() })?,
    };
    let to = match (args.to, &args.output) {
        (Some(t), _) => t,
        (None, Some(Output::Path(p))) => {
            Format::from_path(p).ok_or_else(|| UsageError::UnknownFormat {
                path: p.display().to_string(),
            })?
        }
        (None, Some(Output::Stdout)) => return Err(UsageError::NeedsFormat("to")),
        (None, None) => {
            return Err(UsageError::UnknownFormat {
                path: "(no output given)".into(),
            })
        }
    };

    if !supported(from, to) {
        return Err(UsageError::Unsupported { from, to });
    }
    Ok(Plan { from, to })
}

/// The `--help` text. PDF is the Typst export the GUI calls approximate, and
/// says so here rather than implying it matches print output (ADR 0015).
pub const HELP: &str = "\
suite-convert [OPTIONS] <INPUT> [OUTPUT]

Convert a document without a display. `-` is stdin or stdout.

  -f, --from <FORMAT>     input format; inferred from the extension if omitted
  -t, --to <FORMAT>       output format; inferred from OUTPUT if omitted
  -o, --output <PATH>     output path (or - for stdout); the same as OUTPUT
      --batch <GLOB>      convert every file the quoted glob matches (needs -t and -o DIR)
      --report-loss       print what the conversion lost, as JSON
      --fail-on-loss <L>  exit 4 if the loss exceeds L: none, minor, major or critical
      --page <N>          for png: the page, from 1 (default 1)
      --size <PX>         for png: the width in pixels (default 256)
  -v, --verbose
  -h, --help, -V, --version

PDF output is the in-process Typst export (approximate), not the print path.

Exit codes: 0 ok; 1 bad arguments or missing input; 2 input unreadable as its
format; 3 conversion or I/O failure; 4 converted but over the loss budget.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Command, UsageError> {
        parse(args.iter().map(|a| a.to_string()))
    }

    fn convert(args: &[&str]) -> Args {
        match run(args).unwrap() {
            Command::Convert(a) => a,
            other => panic!("expected a conversion, got {other:?}"),
        }
    }

    #[test]
    fn exit_codes_are_the_published_table() {
        assert_eq!(
            [
                Exit::Ok,
                Exit::Usage,
                Exit::Parse,
                Exit::Convert,
                Exit::Loss
            ]
            .map(Exit::code),
            [0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn positional_input_and_output_infer_both_formats() {
        let args = convert(&["report.docx", "report.pdf"]);
        assert_eq!(
            plan(&args).unwrap(),
            Plan {
                from: Format::Docx,
                to: Format::Pdf
            }
        );
    }

    #[test]
    fn flags_override_the_extensions() {
        let args = convert(&["data.txt", "-f", "csv", "-t", "xlsx", "-o", "out.bin"]);
        assert_eq!(
            plan(&args).unwrap(),
            Plan {
                from: Format::Csv,
                to: Format::Xlsx
            }
        );
    }

    #[test]
    fn dash_is_stdin_and_stdout_and_needs_the_format_said() {
        let args = convert(&["-", "-"]);
        assert_eq!(args.input, Input::Stdin);
        assert_eq!(args.output, Some(Output::Stdout));
        assert_eq!(plan(&args), Err(UsageError::NeedsFormat("from")));
        let args = convert(&["-", "-", "-f", "md"]);
        assert_eq!(plan(&args), Err(UsageError::NeedsFormat("to")));
        let args = convert(&["-", "-", "-f", "md", "-t", "html"]);
        assert_eq!(
            plan(&args).unwrap(),
            Plan {
                from: Format::Md,
                to: Format::Html
            }
        );
    }

    #[test]
    fn option_values_can_be_joined_with_an_equals_sign() {
        let args = convert(&[
            "a.ods",
            "--to=pdf",
            "--fail-on-loss=major",
            "--size=128",
            "--page",
            "2",
        ]);
        assert_eq!(args.to, Some(Format::Pdf));
        assert_eq!(args.fail_on_loss, Some(LossLevel::Major));
        assert_eq!((args.size, args.page), (128, 2));
    }

    #[test]
    fn loss_levels_are_ordered() {
        assert!(
            LossLevel::None < LossLevel::Minor
                && LossLevel::Minor < LossLevel::Major
                && LossLevel::Major < LossLevel::Critical
        );
        assert_eq!(LossLevel::parse("MINOR"), Some(LossLevel::Minor));
        assert_eq!(LossLevel::parse("severe"), None);
    }

    #[test]
    fn batch_takes_a_glob_and_needs_a_target_and_a_directory() {
        let args = convert(&["--batch", "*.docx", "-t", "pdf", "-o", "out/"]);
        assert_eq!(args.input, Input::Batch("*.docx".into()));
        assert_eq!(
            plan(&args).unwrap(),
            Plan {
                from: Format::Docx,
                to: Format::Pdf
            }
        );
        let no_target = convert(&["--batch", "*.docx", "-o", "out/"]);
        assert!(matches!(plan(&no_target), Err(UsageError::BatchNeeds(_))));
        let no_dir = convert(&["--batch", "*.docx", "-t", "pdf"]);
        assert!(matches!(plan(&no_dir), Err(UsageError::BatchNeeds(_))));
        assert_eq!(
            run(&["--batch", "*.docx", "extra.docx"]),
            Err(UsageError::TooManyArguments("extra.docx".into()))
        );
    }

    #[test]
    fn what_converts_to_what_follows_the_spec_table() {
        use Format::*;
        for (from, to) in [
            (Md, Docx),
            (Odt, Pdf),
            (Docx, Html),
            (Csv, Xlsx),
            (Xlsx, Csv),
            (Ods, Pdf),
            (Pptx, Odp),
            (Odp, Png),
        ] {
            assert!(supported(from, to), "{} -> {}", from.name(), to.name());
        }
        for (from, to) in [
            (Docx, Xlsx),
            (Csv, Docx),
            (Pdf, Docx),
            (Png, Pdf),
            (Html, Docx),
            (Pptx, Csv),
            (Csv, Odp),
        ] {
            assert!(!supported(from, to), "{} -> {}", from.name(), to.name());
        }
    }

    #[test]
    fn an_unsupported_conversion_is_a_usage_error() {
        let args = convert(&["a.docx", "b.csv"]);
        assert_eq!(
            plan(&args),
            Err(UsageError::Unsupported {
                from: Format::Docx,
                to: Format::Csv
            })
        );
    }

    #[test]
    fn extensions_are_case_insensitive_and_unknown_ones_are_named() {
        assert_eq!(Format::from_extension("DOCX"), Some(Format::Docx));
        assert_eq!(Format::from_extension("markdown"), Some(Format::Md));
        let args = convert(&["a.weird", "b.pdf"]);
        assert!(matches!(plan(&args), Err(UsageError::UnknownFormat { .. })));
    }

    #[test]
    fn bad_arguments_are_refused_with_a_reason() {
        assert_eq!(run(&[]), Err(UsageError::NoInput));
        assert_eq!(
            run(&["--nope"]),
            Err(UsageError::UnknownOption("--nope".into()))
        );
        assert_eq!(
            run(&["a.docx", "-t"]),
            Err(UsageError::MissingValue("-t".into()))
        );
        assert!(matches!(
            run(&["a.docx", "-t", "exe"]),
            Err(UsageError::BadValue { .. })
        ));
        assert!(matches!(
            run(&["a.docx", "--page", "0"]),
            Err(UsageError::BadValue { .. })
        ));
        assert_eq!(
            run(&["a", "b", "c"]),
            Err(UsageError::TooManyArguments("c".into()))
        );
    }

    #[test]
    fn help_and_version_win_and_double_dash_ends_options() {
        assert_eq!(run(&["a.docx", "--help"]), Ok(Command::Help));
        assert_eq!(run(&["-V"]), Ok(Command::Version));
        let args = convert(&["--", "-weird.docx", "out.pdf"]);
        assert_eq!(args.input, Input::Path("-weird.docx".into()));
    }

    #[test]
    fn the_help_text_names_every_exit_code_and_calls_pdf_approximate() {
        for needle in [
            "0 ok",
            "1 bad arguments",
            "2 input unreadable",
            "3 conversion",
            "4 converted but over",
            "approximate",
            "--fail-on-loss",
        ] {
            assert!(HELP.contains(needle), "{needle}");
        }
    }
}
