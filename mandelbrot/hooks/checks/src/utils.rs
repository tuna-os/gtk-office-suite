use std::{
    fs::{self, File},
    io::{BufRead, BufReader, IsTerminal, Write, stdin, stdout},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use crate::{ScriptError, is_verbose};

/// The data from this script's cargo manifest.
pub(crate) struct CargoManifest {
    /// The name of this script.
    pub(crate) name: String,
    /// The version of this script.
    pub(crate) version: String,
}

impl CargoManifest {
    /// Load the script's cargo manifest data.
    pub(crate) fn load() -> Self {
        let manifest = include_str!("../Cargo.toml");
        let mut name = None;
        let mut version = None;

        for line in manifest.lines().map(str::trim) {
            if let Some(value) = line.strip_prefix("name = ") {
                name = Some(value.trim_matches('"').to_owned())
            } else if let Some(value) = line.strip_prefix("version = ") {
                version = Some(value.trim_matches('"').to_owned())
            }
        }

        Self {
            name: name.expect("name should be in cargo manifest"),
            version: version.expect("version should be in cargo manifest"),
        }
    }
}

/// Files staged for git.
pub(crate) struct GitStagedFiles(Vec<String>);

impl GitStagedFiles {
    /// Load the staged files from git.
    pub(crate) fn load() -> Result<Self, ScriptError> {
        let output = CommandData::new(
            "git",
            &["diff", "--name-only", "--cached", "--diff-filter=d"],
        )
        .run()?;

        if !output.status.success() {
            print_error(&format!(
                "could not get the list of staged files: {}",
                String::from_utf8(output.stderr).expect("git output should be valid UTF-8"),
            ));
            return Err(ScriptError::Check);
        }

        let files = String::from_utf8(output.stdout)
            .expect("git output should be valid UTF-8")
            .trim()
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if files.is_empty() {
            print_error("could not check staged files: no files are staged");
            return Err(ScriptError::Setup);
        }

        Ok(Self(files))
    }

    /// Access the inner slice of this list.
    pub(crate) fn as_slice(&self) -> &[String] {
        &self.0
    }

    /// Whether any of the files in this list match the given predicate.
    pub(crate) fn any<F>(&self, f: F) -> bool
    where
        F: Fn(&str) -> bool,
    {
        self.0.iter().any(|file| f(file.as_str()))
    }

    /// Filter this list with the given predicate.
    pub(crate) fn filter<F>(&self, f: F) -> impl Iterator<Item = &str>
    where
        F: Fn(&str) -> bool,
    {
        self.0.iter().filter_map(move |file| {
            let file = file.as_str();
            f(file).then_some(file)
        })
    }
}

/// A check for the presence of a dependency.
#[derive(Clone, Copy)]
pub(crate) struct CheckDependency {
    /// The name of the dependency.
    pub(crate) name: &'static str,
    /// The command to print the version of the dependency.
    ///
    /// It will be used to check whether the dependency is available.
    pub(crate) version: CommandData,
    /// The command to run to install the dependency.
    pub(crate) install: InstallationCommand,
}

impl CheckDependency {
    /// Check whether the dependency is available.
    ///
    /// Returns `Ok` if the dependency was available or successfully installed.
    pub(crate) fn check(
        self,
        force_install: bool,
        cargo_method: CargoInstallMethod,
    ) -> Result<(), ScriptError> {
        let Self {
            name,
            version,
            install,
        } = self;

        let version = if is_verbose() {
            version.print_output()
        } else {
            version.ignore_output()
        };

        // Do not forward errors here as it might just be the program that is missing.
        if version.run().is_ok_and(|output| output.status.success()) {
            // The dependency is available.
            return Ok(());
        }

        if !force_install {
            self.ask_install(cargo_method)?;
        }

        println!("\x1B[1;92mInstalling\x1B[0m {name}…");

        if install.run(force_install, cargo_method)?.status.success()
            && version.run()?.status.success()
        {
            // The dependency was installed successfully.
            Ok(())
        } else {
            print_error(&format!("could not install {name}",));
            Err(ScriptError::Setup)
        }
    }

    /// Ask the user whether we should try to install the dependency, if we are
    /// in a terminal.
    fn ask_install(self, cargo_method: CargoInstallMethod) -> Result<(), ScriptError> {
        let name = self.name;

        let stdin = stdin();

        if !stdin.is_terminal() {
            print_error(&format!("could not run {name}"));
            return Err(ScriptError::Setup);
        }

        println!("{name} is needed for this check, but it isn’t available\n");
        println!("y: Install {name} via {}", self.install.via(cargo_method));
        println!("N: Don’t install {name} and abort checks\n");

        let mut input = String::new();
        let mut stdout = stdout();

        // Repeat the question until the user selects a proper response.
        loop {
            print!("Install {name}? [y/N]: ");
            stdout.flush().expect("should succeed to flush stdout");

            let mut handle = stdin.lock();
            handle
                .read_line(&mut input)
                .expect("should succeed to read from stdin");

            input = input.trim().to_ascii_lowercase();

            match input.as_str() {
                "y" | "yes" => return Ok(()),
                "n" | "no" | "" => return Err(ScriptError::Setup),
                _ => {
                    println!();
                    print_error("invalid input");
                }
            }

            input.clear();
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CommandData {
    /// The program to execute.
    program: &'static str,
    /// The arguments of the program.
    args: &'static [&'static str],
    /// The behavior for the output of this command.
    output: OutputBehavior,
}

impl CommandData {
    /// Create a new `CommandData` for the given program and arguments.
    ///
    /// By default stdout and stderr will be available in the output of the
    /// command.
    #[must_use]
    pub(crate) fn new(program: &'static str, args: &'static [&'static str]) -> Self {
        Self {
            program,
            args,
            output: OutputBehavior::Read,
        }
    }

    /// Print the output of the command in the shell.
    #[must_use]
    pub(crate) fn print_output(mut self) -> Self {
        self.output = OutputBehavior::Print;
        self
    }

    /// Ignore the output of the command.
    ///
    /// It will neither be printed or read.
    #[must_use]
    pub(crate) fn ignore_output(mut self) -> Self {
        self.output = OutputBehavior::Ignore;
        self
    }

    /// Get the string representation of this command with the given extra
    /// arguments.
    fn to_string_with_args(self, extra_args: &[impl AsRef<str>]) -> String {
        let mut string = self.program.to_owned();

        for arg in self
            .args
            .iter()
            .copied()
            .chain(extra_args.iter().map(AsRef::as_ref))
        {
            string.push(' ');
            string.push_str(arg);
        }

        string
    }

    /// Run this command.
    pub(crate) fn run(self) -> Result<Output, ScriptError> {
        self.run_with_args(&[] as &[&str])
    }

    /// Run this command with the given extra arguments.
    pub(crate) fn run_with_args(
        self,
        extra_args: &[impl AsRef<str>],
    ) -> Result<Output, ScriptError> {
        if is_verbose() {
            println!("\x1B[90m{}\x1B[0m", self.to_string_with_args(extra_args));
        }

        let mut cmd = Command::new(self.program);
        cmd.args(self.args);

        if !extra_args.is_empty() {
            cmd.args(extra_args.iter().map(AsRef::as_ref));
        }

        match self.output {
            OutputBehavior::Read => {}
            OutputBehavior::Print => {
                cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
            }
            OutputBehavior::Ignore => {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }

        cmd.output().map_err(|error| {
            print_error(&format!("could not run command: {error}"));
            ScriptError::Check
        })
    }
}

/// The behavior for the output of a command.
#[derive(Clone, Copy)]
enum OutputBehavior {
    /// Read the output.
    Read,
    /// Print the output in the shell.
    Print,
    /// Ignore the output.
    Ignore,
}

/// The command to use to install a dependency.
#[derive(Clone, Copy)]
pub(crate) enum InstallationCommand {
    /// Use `cargo install` for the given crate.
    Cargo(&'static str),
    /// Use the given command.
    Custom(CommandData),
}

impl InstallationCommand {
    /// The program used for the installation.
    pub(crate) fn via(self, cargo_method: CargoInstallMethod) -> &'static str {
        match self {
            Self::Cargo(_) => cargo_method.name(),
            Self::Custom(cmd) => cmd.program,
        }
    }

    /// Run this command.
    pub(crate) fn run(
        self,
        force_install: bool,
        cargo_method: CargoInstallMethod,
    ) -> Result<Output, ScriptError> {
        match self {
            Self::Cargo(dep) => cargo_method.run(dep, force_install),
            Self::Custom(cmd) => cmd.print_output().run(),
        }
    }
}

/// The method used to install crate dependencies.
#[derive(Clone, Copy, Default)]
pub(crate) enum CargoInstallMethod {
    /// Use `cargo install`.
    #[default]
    Cargo,
    /// Use `cargo-binstall`.
    CargoBinstall,
}

impl CargoInstallMethod {
    /// The name of this method.
    fn name(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::CargoBinstall => "cargo-binstall",
        }
    }

    /// Run tis method for the given dependency.
    fn run(self, dep: &str, force_install: bool) -> Result<Output, ScriptError> {
        if matches!(self, Self::CargoBinstall) {
            CheckDependency {
                name: "cargo-binstall",
                version: CommandData::new("cargo", &["binstall", "-V"]),
                install: InstallationCommand::Cargo("cargo-binstall"),
            }
            .check(force_install, CargoInstallMethod::Cargo)?;
        }

        let cmd = match self {
            Self::Cargo => CommandData::new("cargo", &["install"]),
            Self::CargoBinstall => CommandData::new("cargo", &["binstall"]),
        };

        let mut args = vec![dep];

        if force_install {
            // Both installers no-op when the crate is recorded as installed in
            // `.crates.toml`, which happens in CI when a cache restores cargo's
            // metadata without the binary itself. The version check that
            // follows then fails on a supposedly successful install, so make
            // the install authoritative instead.
            args.push("--force");

            if matches!(self, Self::CargoBinstall) {
                // Non-interactive: never block on the confirmation or the
                // telemetry prompt.
                args.push("--no-confirm");
            }
        }

        cmd.print_output().run_with_args(&args)
    }
}

/// Visit the given directory recursively and apply the given function to files.
pub(crate) fn visit_dir(dir: &Path, on_file: &mut dyn FnMut(PathBuf)) {
    let dir_entries = match fs::read_dir(dir) {
        Ok(dir_entries) => dir_entries,
        Err(error) => {
            print_error(&format!(
                "could not read entries in directory `{}`: {error}",
                dir.to_string_lossy()
            ));
            return;
        }
    };

    for entry in dir_entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                print_error(&format!(
                    "could not read entry in directory `{}`: {error}",
                    dir.to_string_lossy()
                ));
                continue;
            }
        };

        let path = entry.path();

        if path.is_dir() {
            visit_dir(&path, on_file);
        } else {
            on_file(path);
        }
    }
}

/// Whether the given file contains one of the given strings.
///
/// Logs errors when reading a file.
pub(crate) fn file_contains(path: &Path, needles: &[&str]) -> bool {
    let reader = match File::open(path) {
        Ok(file) => BufReader::new(file),
        Err(error) => {
            print_error(&format!(
                "could not open file `{}`: {error}",
                path.to_string_lossy()
            ));
            return false;
        }
    };

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                print_error(&format!(
                    "could not read line of file `{}`: {error}",
                    path.to_string_lossy()
                ));
                return false;
            }
        };

        if needles.iter().any(|needle| line.contains(needle)) {
            return true;
        }
    }

    false
}

/// Load a list of files from the given file under the given base directory.
///
/// The path of the file to load must be relative to the base directory.
///
/// Each file must be on its own line, and lines that start with `#` are
/// ignored. Each file must be relative to the base directory.
///
/// Returns an error if any of the files doesn't exist. The files that don't
/// exist are printed.
pub(crate) fn load_files(base_dir: &Path, file: &Path) -> Result<Vec<PathBuf>, ()> {
    let mut success = true;

    let files = fs::read_to_string(base_dir.join(file))
        .expect("file should be readable")
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|relative_path| {
            let path = base_dir.join(relative_path);

            if !path.exists() {
                print_error(&format!("file `{relative_path}` does not exist"));
                success = false;
            }

            path
        })
        .collect::<Vec<_>>();

    success.then_some(files).ok_or(())
}

/// Check whether the given list of files is ordered alphabetically.
///
/// Returns an error at the first file in the wrong place. The file is printed.
pub(crate) fn check_files_sorted(base_dir: &Path, files: &[PathBuf]) -> Result<(), ()> {
    if let Some((file1, file2)) = files
        .windows(2)
        .find_map(|window| (window[0] > window[1]).then_some((&window[0], &window[1])))
    {
        let relative_file1 = file1
            .strip_prefix(base_dir)
            .expect("all files should be in the base directory")
            .to_owned();
        let relative_file2 = file2
            .strip_prefix(base_dir)
            .expect("all files should be in the base directory")
            .to_owned();

        print_error(&format!(
            "file `{}` before `{}`",
            relative_file1.to_string_lossy(),
            relative_file2.to_string_lossy(),
        ));
        return Err(());
    }

    Ok(())
}

/// Print an error message.
pub(crate) fn print_error(msg: &str) {
    println!("\x1B[91merror:\x1B[0m {msg}");
}
