use std::{
    borrow::Cow,
    collections::BTreeSet,
    env,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    process::{ExitCode, Output},
    sync::{
        LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

mod utils;

use crate::utils::{
    CargoInstallMethod, CargoManifest, CheckDependency, CommandData, GitStagedFiles,
    InstallationCommand, check_files_sorted, file_contains, load_files, print_error, visit_dir,
};

/// The path to the directory containing the workspace.
const WORKSPACE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");

/// The rustup toolchain used for every rustfmt invocation, from
/// `$RUSTFMT_TOOLCHAIN`, defaulting to `nightly`.
///
/// `rustfmt.toml` turns on unstable options — `wrap_comments` and
/// `normalize_comments` among them — and their output is not stable between
/// nightlies: a newer one re-wraps comments an older one left alone. Against a
/// floating `nightly` that makes `fmt --check` fail on files nobody touched,
/// which is not a signal about anyone's change. CI therefore pins a dated
/// nightly through this variable. The default stays plain `nightly` so a
/// contributor needs no extra toolchain to run the hook locally; only the
/// gate that has to give the same answer twice is pinned.
static RUSTFMT_TOOLCHAIN: LazyLock<&'static str> =
    LazyLock::new(|| match env::var("RUSTFMT_TOOLCHAIN") {
        Ok(name) if !name.trim().is_empty() => name.trim().to_owned().leak(),
        _ => "nightly",
    });

/// The `+toolchain` selector naming [`RUSTFMT_TOOLCHAIN`], e.g. `+nightly`.
static RUSTFMT_SELECTOR: LazyLock<&'static str> =
    LazyLock::new(|| &*format!("+{}", *RUSTFMT_TOOLCHAIN).leak());

/// How to reformat the tree, naming the toolchain actually in use so the
/// advice works under a pin as well as under the default.
static RUSTFMT_FIX_HINT: LazyLock<&'static str> = LazyLock::new(|| {
    &*format!(
        "either manually or by running: cargo {} fmt --all",
        *RUSTFMT_SELECTOR
    )
    .leak()
});

/// A `rustup component add` invocation targeting [`RUSTFMT_TOOLCHAIN`].
fn rustup_component_args(component: &'static str) -> &'static [&'static str] {
    vec![
        "component",
        "add",
        "--toolchain",
        *RUSTFMT_TOOLCHAIN,
        component,
    ]
    .leak()
}

/// `args` prefixed with [`RUSTFMT_SELECTOR`], as the `'static` slice
/// [`CommandData`] takes.
fn rustfmt_args(args: &[&'static str]) -> &'static [&'static str] {
    let mut all = Vec::with_capacity(args.len() + 1);
    all.push(*RUSTFMT_SELECTOR);
    all.extend_from_slice(args);
    all.leak()
}

/// Whether to use verbose output.
///
/// Prefer to use [`is_verbose()`], which hides the complexity of the
/// [`AtomicBool`] API.
static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Whether to use verbose output.
///
/// This is helper around the [`VERBOSE`] variable to simplify the API.
fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

fn main() -> ExitCode {
    let result = ScriptCommand::parse_args().and_then(|cmd| cmd.run());
    println!();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => error.exit_code(),
    }
}

/// The possible commands to run in this script.
enum ScriptCommand {
    /// A command that prints the help of this script.
    PrintHelp(PrintHelpCmd),
    /// A command that prints the version of this script.
    PrintVersion(PrintVersionCmd),
    /// A command that runs conformity checks on the current Rust and GTK
    /// project.
    Check(CheckCmd),
}

impl ScriptCommand {
    /// Parse the arguments passed to this script.
    fn parse_args() -> Result<Self, ScriptError> {
        let mut args = std::env::args();
        let _script_path = args
            .next()
            .expect("the first argument to the script should be its path");

        let mut check_cmd = CheckCmd::default();
        let mut git_staged = false;

        for arg in args {
            if let Some(long_flag) = arg.strip_prefix("--") {
                match long_flag {
                    "git-staged" => {
                        git_staged = true;
                    }
                    "force-install" => {
                        check_cmd.force_install = true;
                    }
                    "cargo-binstall" => {
                        check_cmd.cargo_install_method = CargoInstallMethod::CargoBinstall;
                    }
                    "verbose" => {
                        VERBOSE.store(true, Ordering::Relaxed);
                    }
                    "version" => {
                        return Ok(Self::PrintVersion(PrintVersionCmd));
                    }
                    "help" => {
                        return Ok(Self::PrintHelp(PrintHelpCmd));
                    }
                    _ => {
                        print_error(&format!("unsupported flag `--{long_flag}`"));
                        PrintHelpCmd.run();
                        return Err(ScriptError::Check);
                    }
                }
            } else if let Some(short_flags) = arg.strip_prefix('-') {
                // We allow to combine short flags.
                for short_flag in short_flags.chars() {
                    match short_flag {
                        's' => {
                            git_staged = true;
                        }
                        'f' => {
                            check_cmd.force_install = true;
                        }
                        'v' => {
                            VERBOSE.store(true, Ordering::Relaxed);
                        }
                        'h' => {
                            return Ok(Self::PrintHelp(PrintHelpCmd));
                        }
                        _ => {
                            print_error(&format!("unsupported flag `-{short_flag}`"));
                            PrintHelpCmd.run();
                            return Err(ScriptError::Check);
                        }
                    }
                }
            } else {
                print_error(&format!("unsupported argument `{arg}`"));
                PrintHelpCmd.run();
                return Err(ScriptError::Check);
            }
        }

        if git_staged {
            check_cmd.staged_files = Some(GitStagedFiles::load()?);
        }

        Ok(Self::Check(check_cmd))
    }

    /// Run the current command.
    fn run(self) -> Result<(), ScriptError> {
        match self {
            Self::PrintHelp(cmd) => cmd.run(),
            Self::PrintVersion(cmd) => cmd.run(),
            Self::Check(cmd) => cmd.run()?,
        }

        Ok(())
    }
}

/// A command that prints the help of this script.
struct PrintHelpCmd;

impl PrintHelpCmd {
    /// Run this command.
    fn run(self) {
        let CargoManifest { name, .. } = CargoManifest::load();
        println!(
            "\
Run conformity checks on the current Rust project.

If a dependency is not found, helps the user to install it.

USAGE: {name} [OPTIONS]

OPTIONS:
    -s, --git-staged        Only check files staged to be committed
    -f, --force-install     Install missing dependencies without asking
    --cargo-binstall        Use cargo-binstall instead of `cargo install` when installing
                            missing crate dependencies
    -v, --verbose           Use verbose output
    --version               Print the version of this script
    -h, --help              Print this help and exit

ERROR CODES:
    1                       Check failed
    2                       Setup failed
",
        );
    }
}

/// A command that prints the version of this script.
struct PrintVersionCmd;

impl PrintVersionCmd {
    /// Run this command.
    fn run(self) {
        let CargoManifest { name, version } = CargoManifest::load();
        println!("{name} {version}");
    }
}

/// A command that runs conformity checks on the current Rust and GTK project.
#[derive(Default)]
struct CheckCmd {
    /// The staged files to check.
    ///
    /// If this is `None`, all files are checked.
    staged_files: Option<GitStagedFiles>,
    /// Whether to install missing dependencies without asking.
    force_install: bool,
    /// Which program to use to install crate dependencies.
    cargo_install_method: CargoInstallMethod,
}

impl CheckCmd {
    /// Run this command.
    fn run(self) -> Result<(), ScriptError> {
        self.code_style()?;
        self.lint_script()?;
        self.spelling()?;
        self.unused_dependencies()?;
        self.allowed_dependencies()?;
        self.potfiles()?;
        self.blueprints()?;
        self.data_gresources()?;
        self.cargo_manifest()?;
        self.markdown()?;

        Ok(())
    }

    /// Check code style with rustfmt nightly.
    fn code_style(&self) -> Result<(), ScriptError> {
        let mut check = Check::start("code style").with_fix(*RUSTFMT_FIX_HINT);

        CheckDependency {
            name: "rustfmt",
            version: CommandData::new("cargo", rustfmt_args(&["fmt", "--version"])),
            install: InstallationCommand::Custom(CommandData::new(
                "rustup",
                rustup_component_args("rustfmt"),
            )),
        }
        .check(self.force_install, self.cargo_install_method)?;

        if let Some(staged_files) = &self.staged_files {
            let cmd = CommandData::new(
                "cargo",
                rustfmt_args(&[
                    "fmt",
                    "--check",
                    "--",
                    "--unstable-features",
                    "--skip-children",
                ]),
            )
            .print_output();

            for rust_file in staged_files.filter(|file| file.ends_with(".rs")) {
                let output = cmd.run_with_args(&[rust_file])?;
                check.merge_output(output);
            }
        } else {
            let output = CommandData::new("cargo", rustfmt_args(&["fmt", "--check", "--all"]))
                .print_output()
                .run()?;
            check.merge_output(output);
        }

        check.end()
    }

    /// Lint this crate with clippy and rustfmt.
    fn lint_script(&self) -> Result<(), ScriptError> {
        if self
            .staged_files
            .as_ref()
            .is_some_and(|staged_files| !staged_files.any(|file| file.starts_with("hooks/checks")))
        {
            // No check necessary.
            return Ok(());
        }

        let mut check = Check::start("hooks/checks");

        CheckDependency {
            name: "clippy",
            version: CommandData::new("cargo", &["clippy", "--version"]),
            install: InstallationCommand::Custom(CommandData::new(
                "rustup",
                &["component", "add", "clippy"],
            )),
        }
        .check(self.force_install, self.cargo_install_method)?;

        let manifest_path = format!("{WORKSPACE_DIR}/hooks/checks/Cargo.toml");

        let output = CommandData::new("cargo", &["clippy", "--all-targets", "--manifest-path"])
            .print_output()
            .run_with_args(&[&manifest_path])?;
        check.merge_output(output);

        // We should have already checked that rustfmt is installed.
        let output = CommandData::new(
            "cargo",
            rustfmt_args(&["fmt", "--check", "--manifest-path"]),
        )
        .print_output()
        .run_with_args(&[&manifest_path])?;
        check.merge_output(output);

        check.end()
    }

    /// Check spelling with typos.
    fn spelling(&self) -> Result<(), ScriptError> {
        let mut check =
            Check::start("spelling mistakes").with_fix("either manually or by running: typos -w");

        CheckDependency {
            name: "typos",
            version: CommandData::new("typos", &["--version"]),
            install: InstallationCommand::Cargo("typos-cli"),
        }
        .check(self.force_install, self.cargo_install_method)?;

        let cmd = CommandData::new("typos", &["--color", "always"]).print_output();

        let output = if let Some(staged_files) = &self.staged_files {
            cmd.run_with_args(staged_files.as_slice())?
        } else {
            cmd.run()?
        };

        check.merge_output(output);
        check.end()
    }

    /// Check unused dependencies with cargo-machete.
    fn unused_dependencies(&self) -> Result<(), ScriptError> {
        let mut check = Check::start("unused dependencies").with_fix(
            "either by removing the dependencies, or by adding \
             the necessary configuration option in Cargo.toml \
             (see cargo-machete documentation)",
        );

        CheckDependency {
            name: "cargo-machete",
            version: CommandData::new("cargo-machete", &["--version"]),
            install: InstallationCommand::Cargo("cargo-machete"),
        }
        .check(self.force_install, self.cargo_install_method)?;

        let output = CommandData::new("cargo-machete", &["--with-metadata"])
            .print_output()
            .run()?;

        check.merge_output(output);
        check.end()
    }

    /// Check allowed dependencies with cargo-deny.
    fn allowed_dependencies(&self) -> Result<(), ScriptError> {
        let mut check = Check::start("allowed dependencies").with_fix(
            "either by removing the dependencies, or by adding \
             the necessary configuration option in deny.toml \
             (see cargo-deny documentation)",
        );

        CheckDependency {
            name: "cargo-deny",
            version: CommandData::new("cargo", &["deny", "--version"]),
            install: InstallationCommand::Cargo("cargo-deny"),
        }
        .check(self.force_install, self.cargo_install_method)?;

        // `--all-features` and not a bare `check`: `calls-media` is off by
        // default and on in the shipped Flatpak (org.tunaos.mandelbrot.json
        // builds with `-Dcalls-media=true`), so a default-feature audit skips
        // the whole LiveKit/libwebrtc subtree — the media stack that actually
        // reaches users, and the part of the tree handling untrusted network
        // media. It stayed green while rtrb 0.3.4 carried RUSTSEC-2026-0274.
        let output = CommandData::new("cargo", &["deny", "--all-features", "check"])
            .print_output()
            .run()?;

        check.merge_output(output);
        check.end()
    }

    /// Check that files in `POTFILES.in` and `POTFILES.skip` are correct.
    ///
    /// This applies the following checks, in that order:
    ///
    /// - All listed files exist
    /// - All files with translatable strings are listed and only those
    /// - Listed files are sorted alphabetically
    /// - No Rust files use the gettext-rs macros
    ///
    /// This assumes the following:
    ///
    /// - The POTFILES are located at `po/POTFILES.(in/skip)`.
    /// - UI (GtkBuilder) files are located under `src` and use
    ///   `translatable="yes"`.
    /// - Blueprint files are located under `src` and use `_(`.
    /// - Rust files are located under `src` and use `*gettext(_f)` methods.
    fn potfiles(&self) -> Result<(), ScriptError> {
        let base_dir = Path::new(WORKSPACE_DIR);

        let potfiles_in_exist_check = Check::start("files exist in po/POTFILES.in");
        let Ok(potfiles_in) = load_files(base_dir, Path::new("po/POTFILES.in")) else {
            return potfiles_in_exist_check.fail();
        };
        potfiles_in_exist_check.end()?;

        let potfiles_in_order_check =
            Check::start("files are ordered alphabetically in po/POTFILES.in");
        if check_files_sorted(base_dir, &potfiles_in).is_err() {
            return potfiles_in_order_check.fail();
        }
        potfiles_in_order_check.end()?;

        let potfiles_skip_exist_check = Check::start("files exist in po/POTFILES.skip");
        let Ok(potfiles_skip) = load_files(base_dir, Path::new("po/POTFILES.skip")) else {
            return potfiles_skip_exist_check.fail();
        };
        potfiles_skip_exist_check.end()?;

        let potfiles_skip_order_check =
            Check::start("files are ordered alphabetically in po/POTFILES.skip");
        if check_files_sorted(base_dir, &potfiles_skip).is_err() {
            return potfiles_skip_order_check.fail();
        }
        potfiles_skip_order_check.end()?;

        let mut translatable_files_check = Check::start(
            "all files with translatable strings are present in po/POTFILES.in or po/POTFILES.skip",
        );
        let mut translatable_ui = BTreeSet::new();
        let mut translatable_blp = BTreeSet::new();
        let mut translatable_rs = BTreeSet::new();

        visit_dir(&base_dir.join("src"), &mut |path| {
            if potfiles_skip.contains(&path) {
                return;
            }
            let Some(extension) = path.extension() else {
                return;
            };

            if extension == "ui" {
                if file_contains(&path, &[r#"translatable="yes""#]) {
                    translatable_ui.insert(path);
                }
            } else if extension == "blp" {
                if file_contains(&path, &["_("]) {
                    translatable_blp.insert(path);
                }
            } else if extension == "rs" {
                if file_contains(&path, &["gettext!("]) {
                    let relative_path = path
                        .strip_prefix(base_dir)
                        .expect("all visited files should be in the workspace")
                        .to_owned();

                    print_error(&format!(
                        "file '{}' uses a gettext-rs macro, use the corresponding i18n method instead",
                        relative_path.to_string_lossy(),
                    ));
                    translatable_files_check.record_failure();
                }
                if file_contains(&path, &["gettext(", "gettext_f("]) {
                    translatable_rs.insert(path);
                }
            }
        });

        let mut potfiles_in_ui = BTreeSet::new();
        let mut potfiles_in_blp = BTreeSet::new();
        let mut potfiles_in_rs = BTreeSet::new();

        for path in potfiles_in {
            let Some(extension) = path.extension() else {
                continue;
            };

            if extension == "ui" {
                potfiles_in_ui.insert(path);
            } else if extension == "blp" {
                potfiles_in_blp.insert(path);
            } else if extension == "rs" {
                potfiles_in_rs.insert(path);
            }
        }

        let not_translatable = potfiles_in_ui
            .difference(&translatable_ui)
            .chain(potfiles_in_blp.difference(&translatable_blp))
            .chain(potfiles_in_rs.difference(&translatable_rs))
            .collect::<Vec<_>>();
        if !not_translatable.is_empty() {
            translatable_files_check.record_failure();
            let count = not_translatable.len();

            if count == 1 {
                print_error("Found 1 file with translatable strings not present in POTFILES.in:");
            } else {
                print_error(&format!(
                    "Found {count} files with translatable strings not present in POTFILES.in:"
                ));
            }
        }
        for path in not_translatable {
            let relative_path = path
                .strip_prefix(base_dir)
                .expect("all visited files should be in the workspace")
                .to_owned();

            println!("{}", relative_path.to_string_lossy());
        }

        let missing_translatable = translatable_ui
            .difference(&potfiles_in_ui)
            .chain(translatable_blp.difference(&potfiles_in_blp))
            .chain(translatable_rs.difference(&potfiles_in_rs))
            .collect::<Vec<_>>();
        if !missing_translatable.is_empty() {
            translatable_files_check.record_failure();
            let count = missing_translatable.len();

            if count == 1 {
                print_error("Found 1 file in POTFILES.in without translatable strings:");
            } else {
                print_error(&format!(
                    "Found {count} files in POTFILES.in without translatable strings:"
                ));
            }
        }
        for path in missing_translatable {
            let relative_path = path
                .strip_prefix(base_dir)
                .expect("all visited files should be in the workspace")
                .to_owned();

            println!("{}", relative_path.to_string_lossy());
        }

        translatable_files_check.end()
    }

    /// Check `src/ui-blueprint-resources.in`.
    ///
    /// Checks that the files exist and are sorted alphabetically.
    fn blueprints(&self) -> Result<(), ScriptError> {
        let base_dir = Path::new(WORKSPACE_DIR).join("src");

        if self.staged_files.as_ref().is_some_and(|staged_files| {
            !staged_files.any(|file| file == "src/ui-blueprint-resources.in")
        }) {
            // No check necessary.
            return Ok(());
        }

        let files_exist_check = Check::start("files exist in src/ui-blueprint-resources.in");
        let Ok(blueprint_files) = load_files(&base_dir, Path::new("ui-blueprint-resources.in"))
        else {
            return files_exist_check.fail();
        };
        files_exist_check.end()?;

        let files_order_check =
            Check::start("files are ordered alphabetically in src/ui-blueprint-resources.in");
        if check_files_sorted(&base_dir, &blueprint_files).is_err() {
            return files_order_check.fail();
        }
        files_order_check.end()
    }

    /// Check that files listed in `data/resources/resources.gresource.xml` are
    /// sorted alphabetically.
    fn data_gresources(&self) -> Result<(), ScriptError> {
        const GRESOURCES_PATH: &str = "data/resources/resources.gresource.xml";

        if self
            .staged_files
            .as_ref()
            .is_some_and(|staged_files| !staged_files.any(|file| file == GRESOURCES_PATH))
        {
            // No check necessary.
            return Ok(());
        }

        let check = Check::start(
            "files are ordered alphabetically in data/resources/resources.gresource.xml",
        );

        let reader = match File::open(Path::new(WORKSPACE_DIR).join(GRESOURCES_PATH)) {
            Ok(file) => BufReader::new(file),
            Err(error) => {
                print_error(&format!("could not open file `{GRESOURCES_PATH}`: {error}"));
                return check.fail();
            }
        };

        let mut previous_file_path: Option<String> = None;

        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    print_error(&format!(
                        "could not read line of file `{GRESOURCES_PATH}`: {error}"
                    ));
                    return check.fail();
                }
            };

            // The file path is between `<file*>` and `</file>`.
            let Some((file_path, _)) = line
                .split_once("<file")
                .and_then(|(_, line_end)| line_end.split_once('>'))
                .and_then(|(_, line_end)| line_end.split_once("</file>"))
            else {
                continue;
            };

            if let Some(previous_file_path) = previous_file_path.as_deref() {
                if previous_file_path > file_path {
                    print_error(&format!("file `{previous_file_path}` before `{file_path}`"));
                    return check.fail();
                }
            }

            previous_file_path = Some(file_path.to_owned());
        }

        check.end()
    }

    /// Check `Cargo.toml` with cargo-sort.
    fn cargo_manifest(&self) -> Result<(), ScriptError> {
        if self
            .staged_files
            .as_ref()
            .is_some_and(|staged_files| !staged_files.any(|file| file == "Cargo.toml"))
        {
            // No check necessary.
            return Ok(());
        }

        let mut check = Check::start("Cargo.toml sorting").with_fix(
            "either manually or by running: cargo-sort --grouped --order \
             package,lib,profile,features,dependencies,target,dev-dependencies,build-dependencies",
        );

        CheckDependency {
            name: "cargo-sort",
            version: CommandData::new("cargo", &["sort", "--version"]),
            install: InstallationCommand::Cargo("cargo-sort"),
        }
        .check(self.force_install, self.cargo_install_method)?;

        let output = CommandData::new(
            "cargo",
            &["sort", "--check", "--grouped", "--order", "workspace,package,lib,profile,features,dependencies,target,dev-dependencies,build-dependencies"]
        ).print_output().run()?;

        check.merge_output(output);
        check.end()
    }

    /// Lint markdown files.
    fn markdown(&self) -> Result<(), ScriptError> {
        let staged_markdown_files = self.staged_files.as_ref().map(|staged_files| {
            staged_files
                .filter(|file| file.ends_with(".md"))
                .collect::<Vec<_>>()
        });

        if staged_markdown_files.as_ref().is_some_and(Vec::is_empty) {
            // No check necessary.
            return Ok(());
        }

        let mut check = Check::start("markdown files")
            .with_fix("either manually or by running: rumdl check --fix .");

        CheckDependency {
            name: "rumdl",
            version: CommandData::new("rumdl", &["--version"]),
            install: InstallationCommand::Cargo("rumdl"),
        }
        .check(self.force_install, self.cargo_install_method)?;

        if let Some(staged_markdown_files) = staged_markdown_files {
            let output = CommandData::new("rumdl", &["check"])
                .print_output()
                .run_with_args(&staged_markdown_files)?;
            check.merge_output(output);
        } else {
            let output = CommandData::new("rumdl", &["check", "."])
                .print_output()
                .run()?;
            check.merge_output(output);
        }

        check.end()
    }
}

/// A check in this script.
struct Check {
    /// The name of this check.
    name: &'static str,
    /// The way to fix a failure of this check.
    fix: Option<Cow<'static, str>>,
    /// Whether this check was successful.
    success: bool,
}

impl Check {
    /// Start the check with the given name.
    fn start(name: &'static str) -> Self {
        println!("\n\x1B[1;92mChecking\x1B[0m {name}");

        Self {
            name,
            fix: None,
            success: true,
        }
    }

    /// Set the way to fix a failure of this check.
    fn with_fix(mut self, fix: impl Into<Cow<'static, str>>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    /// Record the failure of this check.
    fn record_failure(&mut self) {
        self.success = false;
    }

    /// Merge the given output for the result of this check.
    fn merge_output(&mut self, output: Output) {
        self.success &= output.status.success();
    }

    /// Finish this check.
    ///
    /// Print the result and convert it to a Rust result.
    fn end(self) -> Result<(), ScriptError> {
        let Self { name, fix, success } = self;

        if success {
            println!("Checking {name} result: \x1B[1;92mok\x1B[0m",);
            Ok(())
        } else {
            println!("Checking {name} result: \x1B[1;91mfail\x1B[0m",);

            if let Some(fix) = fix {
                println!("Please fix the above issues, {fix}");
            } else {
                println!("Please fix the above issues");
            }

            Err(ScriptError::Check)
        }
    }

    /// Fail this check immediately.
    fn fail(mut self) -> Result<(), ScriptError> {
        self.record_failure();
        self.end()
    }
}

/// The possible errors returned by this script.
enum ScriptError {
    /// A check failed.
    Check,
    /// The setup for a check failed.
    Setup,
}

impl ScriptError {
    /// The exit code to return for this error.
    fn exit_code(&self) -> ExitCode {
        match self {
            Self::Check => 1,
            Self::Setup => 2,
        }
        .into()
    }
}
