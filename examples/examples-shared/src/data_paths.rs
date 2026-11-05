/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! Resolve the on-premise Device Detection and IP Intelligence data files.
//!
//! Both resolvers prefer an explicit environment-variable path and otherwise
//! fall back to the free data files in the data repository, which the `cxx`
//! submodule holds at `cxx/data`. [`find_data_file`] locates each one with
//! [`crate::find_file`] and runs the data repository's script for a file that
//! is missing, the PowerShell script on Windows and the shell script elsewhere.
//! The IP Intelligence resolver implements the tier scheme (Enterprise, ASN,
//! Lite) that the Rust examples use.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};

use crate::find_file::find_file_from;

/// The environment variable giving an explicit Device Detection data-file path.
pub const DD_PATH_ENV_VAR: &str = "51DEGREES_DD_PATH";

/// The environment variable giving an explicit IP Intelligence data-file path.
pub const IPI_PATH_ENV_VAR: &str = "51DEGREES_IPI_PATH";

/// The Lite Device Detection data file in the data repository.
pub const DD_LITE_RELATIVE_PATH: &str = "cxx/data/device-detection/51Degrees-LiteV4.1.hash";

/// The ASN IP Intelligence data file in the data repository, giving Asn and
/// AsnName.
///
/// It is a download of a few megabytes, so it is the default for the examples,
/// their tests and CI.
pub const IPI_ASN_RELATIVE_PATH: &str = "cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi";

/// The Lite IP Intelligence data file in the data repository, giving
/// RegisteredName, RegisteredOwner and RegisteredCountry.
///
/// It is a download of a few hundred megabytes, so only the examples that show
/// those properties ask for it.
pub const IPI_LITE_RELATIVE_PATH: &str = "cxx/data/ip-intelligence/51Degrees-LiteV41.ipi";

/// The Enterprise IP Intelligence data-file name on the Enterprise share.
pub const IPI_ENTERPRISE_FILE_NAME: &str = "51Degrees-IPIV4EnterpriseIpiV41.ipi";

/// The environment variable giving the root of the dated Enterprise IP
/// Intelligence share, which holds the latest data file under a `YYYY/MM/DD`
/// subdirectory. Unset by default, so the Enterprise tier is skipped unless a
/// deployment points this at a reachable share.
pub const IPI_ENTERPRISE_SHARE_ENV_VAR: &str = "51DEGREES_IPI_ENTERPRISE_SHARE";

/// Which tier of IP Intelligence data file to resolve.
///
/// The default, [`IpiTier::BestAvailable`], picks the best *loadable* tier:
/// Enterprise when the configured share is reachable, otherwise ASN. A caller
/// can force a specific tier when an example needs it, for example to exercise
/// the Enterprise-only properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IpiTier {
    /// Pick the best loadable tier: Enterprise if the share is reachable,
    /// otherwise the ASN file from the data repository.
    #[default]
    BestAvailable,
    /// Force the ASN file ([`IPI_ASN_RELATIVE_PATH`]). Small, current and
    /// free from the data repository, so it suits most quick tests.
    Asn,
    /// Force the Lite file ([`IPI_LITE_RELATIVE_PATH`]), free from the data
    /// repository and much larger than the ASN file.
    Lite,
    /// Force the Enterprise file from the Enterprise share. Resolves to
    /// `None` when the share is not reachable.
    Enterprise,
}

/// Resolve the path to an on-premise Device Detection data file.
///
/// Checks [`DD_PATH_ENV_VAR`] first and returns its value when set to a non-blank
/// path. Otherwise finds [`DD_LITE_RELATIVE_PATH`] with [`find_data_file`],
/// which downloads it when it is missing.
/// Returns `None` if neither is available.
pub fn dd_data_path() -> Option<PathBuf> {
    if let Some(path) = env_path(DD_PATH_ENV_VAR) {
        return Some(path);
    }
    find_data_file(DD_LITE_RELATIVE_PATH)
}

/// Resolve the path to an on-premise IP Intelligence data file at the requested
/// tier.
///
/// [`IPI_PATH_ENV_VAR`] always wins when set to a non-blank path, regardless of
/// `tier`, so a caller can point every example at one explicit file. Otherwise
/// the tier decides:
///
/// - [`IpiTier::BestAvailable`] returns the latest Enterprise file from the
///   configured share if it is reachable, and otherwise the ASN file.
/// - [`IpiTier::Enterprise`] returns the latest Enterprise file, or `None` when
///   the share is unreachable.
/// - [`IpiTier::Asn`] and [`IpiTier::Lite`] return that free file, found with
///   [`find_data_file`], which downloads it when it is missing.
///
/// Returns `None` when no file for the requested tier can be located.
pub fn ipi_data_path(tier: IpiTier) -> Option<PathBuf> {
    if let Some(path) = env_path(IPI_PATH_ENV_VAR) {
        return Some(path);
    }
    match tier {
        IpiTier::BestAvailable => {
            latest_enterprise_ipi().or_else(|| find_data_file(IPI_ASN_RELATIVE_PATH))
        }
        IpiTier::Enterprise => latest_enterprise_ipi(),
        IpiTier::Asn => find_data_file(IPI_ASN_RELATIVE_PATH),
        IpiTier::Lite => find_data_file(IPI_LITE_RELATIVE_PATH),
    }
}

/// The `source` the examples send with each download, so that downloads can be
/// counted by where they come from.
pub const DATA_DOWNLOAD_SOURCE: &str = "examples-51degrees-rust";

/// The module path Windows PowerShell must not inherit from PowerShell 7. With
/// PowerShell 7's module folders it cannot load its own modules, and the data
/// repository's script stops because Get-FileHash is not recognised.
const POWERSHELL_MODULE_PATH_ENV_VAR: &str = "PSModulePath";

/// The links this process has already run the download script for. Each file
/// is tried once, and the lock means two threads never download one together.
static DOWNLOADS_TRIED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Find a data file the data repository links to, downloading it when it is
/// missing.
///
/// `relative_path` is looked for as [`crate::find_file`] looks for it. The data
/// repository lists each data file as `<file>.link` in the folder where the
/// file goes, so when the file is missing and its link is there, the data
/// repository is the folder above, and its script is run for that one file,
/// saying so. The file is then looked for again. When it still cannot be found,
/// a message naming the command that gets it is printed and `None` is returned.
pub fn find_data_file(relative_path: &str) -> Option<PathBuf> {
    let start = std::env::current_dir().ok()?;
    find_data_file_from(relative_path, &start)
}

/// Find a data file the data repository links to by walking up from `start`
/// instead of the current working directory, downloading it when it is
/// missing. See [`find_data_file`].
pub fn find_data_file_from(relative_path: &str, start: &Path) -> Option<PathBuf> {
    find_data_file_for(relative_path, start, Host::current())
}

/// [`find_data_file_from`] with the download command chosen for `host`.
fn find_data_file_for(relative_path: &str, start: &Path, host: Host) -> Option<PathBuf> {
    if let Some(found) = find_file_from(relative_path, start) {
        return Some(found);
    }
    let name = Path::new(relative_path).file_name()?.to_str()?;
    let mut tried = DOWNLOADS_TRIED
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // Another thread may have downloaded the file while this one waited.
    if let Some(found) = find_file_from(relative_path, start) {
        return Some(found);
    }

    let Some(link) = find_file_from(format!("{relative_path}.link"), start) else {
        // Without the link the data repository is not checked out here, so the
        // command is given relative to the root of this repository.
        let data_repository = Path::new(relative_path).parent()?.parent()?;
        let command = DownloadCommand::for_host(data_repository, name, host);
        eprintln!(
            "{name} was not found. Run these commands from the root of the \
             51degrees-rust repository to get it.\n\
             git submodule update --init --recursive\n{command}"
        );
        return None;
    };
    let data_repository = link.parent()?.parent()?;
    let command = DownloadCommand::for_host(data_repository, name, host);
    if !tried.contains(&link) {
        tried.push(link.clone());
        eprintln!("{name} is missing, so it is being downloaded by running\n{command}");
        if command.run() {
            if let Some(found) = find_file_from(relative_path, start) {
                return Some(found);
            }
        }
    }
    eprintln!("{name} could not be downloaded. Run this command to try again.\n{command}");
    None
}

/// What the download command depends on besides the file. [`Host::current`]
/// reads it from this process, and the tests give their own.
#[derive(Debug, Clone, Copy)]
struct Host {
    /// Whether this is Windows, where the PowerShell script is run.
    windows: bool,
    /// Whether PowerShell 7, `pwsh`, is on the PATH.
    has_pwsh: bool,
}

impl Host {
    /// The host this process runs on.
    fn current() -> Self {
        let windows = cfg!(windows);
        Host {
            windows,
            has_pwsh: windows && on_path("pwsh"),
        }
    }
}

/// Whether `program`, or `program.exe`, is in a folder on the PATH.
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|folder| {
            folder.join(program).is_file() || folder.join(format!("{program}.exe")).is_file()
        })
    })
}

/// The command that runs the data repository's download script for one file.
/// On Windows that is the PowerShell script, in PowerShell 7 when it is on the
/// PATH and otherwise in Windows PowerShell without the module path it would
/// inherit from PowerShell 7. Everywhere else it is the shell script in `sh`.
struct DownloadCommand {
    program: &'static str,
    arguments: Vec<String>,
    /// Variables removed from the environment the command runs in.
    removed: Vec<&'static str>,
}

impl DownloadCommand {
    /// The command that downloads `name` into the data repository at
    /// `data_repository` on `host`.
    fn for_host(data_repository: &Path, name: &str, host: Host) -> Self {
        let folder = command_path(data_repository);
        if host.windows {
            let (program, removed) = if host.has_pwsh {
                ("pwsh", Vec::new())
            } else {
                ("powershell", vec![POWERSHELL_MODULE_PATH_ENV_VAR])
            };
            let arguments = vec![
                "-NoProfile".to_owned(),
                "-ExecutionPolicy".to_owned(),
                "Bypass".to_owned(),
                "-File".to_owned(),
                format!("{folder}\\get-data-files.ps1"),
                name.to_owned(),
                "-Source".to_owned(),
                DATA_DOWNLOAD_SOURCE.to_owned(),
            ];
            Self {
                program,
                arguments,
                removed,
            }
        } else {
            let arguments = vec![
                format!("{folder}/get-data-files.sh"),
                "-s".to_owned(),
                DATA_DOWNLOAD_SOURCE.to_owned(),
                name.to_owned(),
            ];
            Self {
                program: "sh",
                arguments,
                removed: Vec::new(),
            }
        }
    }

    /// Run the command, returning whether it succeeded. The script's progress
    /// goes to standard error, as this module's own messages do, so the
    /// example's standard output carries only its results.
    fn run(&self) -> bool {
        let mut command = Command::new(self.program);
        command.args(&self.arguments).stdout(std::io::stderr());
        for name in &self.removed {
            command.env_remove(name);
        }
        command.status().is_ok_and(|status| status.success())
    }
}

/// The command as it would be typed, with each argument that holds anything
/// other than letters, digits, dots, dashes and underscores in double quotes,
/// followed by any variable removed from its environment.
impl fmt::Display for DownloadCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.program)?;
        for argument in &self.arguments {
            let plain = !argument.is_empty()
                && argument
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
            if plain {
                write!(f, " {argument}")?;
            } else {
                write!(f, " \"{argument}\"")?;
            }
        }
        for name in &self.removed {
            write!(f, " (with {name} removed from its environment)")?;
        }
        Ok(())
    }
}

/// `path` as it would be typed in a command. On Windows the `\\?\` prefix that
/// `canonicalize` adds is removed and every separator is a backslash.
fn command_path(path: &Path) -> String {
    let text = path.display().to_string();
    if !cfg!(windows) {
        return text;
    }
    let text = if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(local) = text.strip_prefix(r"\\?\") {
        local.to_owned()
    } else {
        text
    };
    text.replace('/', "\\")
}

/// Locate the latest dated Enterprise IP Intelligence file on the configured
/// share ([`IPI_ENTERPRISE_SHARE_ENV_VAR`]), or `None` when the share is unset
/// or not reachable.
///
/// The share is laid out as `<root>/YYYY/MM/DD/<file>`. The most recent dated
/// folder that actually contains the file is chosen, comparing folder names
/// lexicographically (zero-padded `YYYY`, `MM` and `DD` sort the same as by
/// date). When the variable is unset, or any I/O error occurs (the common case
/// when the share is not mounted), the result is `None`, so the resolver never
/// blocks or panics.
pub fn latest_enterprise_ipi() -> Option<PathBuf> {
    let root = env_path(IPI_ENTERPRISE_SHARE_ENV_VAR)?;
    let year = latest_numeric_child(&root)?;
    let month = latest_numeric_child(&year)?;
    let day = latest_dated_day_with_file(&month)?;
    let file = day.join(IPI_ENTERPRISE_FILE_NAME);
    file.exists().then_some(file)
}

/// The immediate subdirectories of `dir` whose names are all ASCII digits,
/// sorted ascending by name (zero-padded `YYYY`, `MM` and `DD` sort the same as
/// by date). Returns an empty `Vec` when `dir` cannot be read.
fn numeric_child_dirs(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Ok(name) = entry.file_name().into_string() {
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
                dirs.push((name, path));
            }
        }
    }
    dirs.sort_by(|(a, _), (b, _)| a.cmp(b));
    dirs.into_iter().map(|(_, path)| path).collect()
}

/// The lexicographically greatest immediate subdirectory of `dir` whose name is
/// all digits, or `None` if there is none (or `dir` cannot be read).
fn latest_numeric_child(dir: &std::path::Path) -> Option<PathBuf> {
    numeric_child_dirs(dir).into_iter().next_back()
}

/// The latest day subdirectory of `month` that actually contains the Enterprise
/// data file. Falling back from the latest empty day to an earlier populated one
/// keeps the resolver working on a day the upload has not yet completed.
fn latest_dated_day_with_file(month: &std::path::Path) -> Option<PathBuf> {
    numeric_child_dirs(month)
        .into_iter()
        .rev()
        .find(|day| day.join(IPI_ENTERPRISE_FILE_NAME).exists())
}

/// Read `name` from the environment as a path, returning it only when set to a
/// non-blank value (the value is trimmed and rejected when empty).
fn env_path(name: &str) -> Option<PathBuf> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(PathBuf::from(value.trim())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn dd_env_var_wins() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(DD_PATH_ENV_VAR, "/tmp/explicit-dd.hash");
        assert_eq!(dd_data_path(), Some(PathBuf::from("/tmp/explicit-dd.hash")));
        std::env::remove_var(DD_PATH_ENV_VAR);
    }

    #[test]
    fn ipi_env_var_wins_for_every_tier() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(IPI_PATH_ENV_VAR, "/tmp/explicit.ipi");
        for tier in [IpiTier::BestAvailable, IpiTier::Asn, IpiTier::Enterprise] {
            assert_eq!(
                ipi_data_path(tier),
                Some(PathBuf::from("/tmp/explicit.ipi"))
            );
        }
        std::env::remove_var(IPI_PATH_ENV_VAR);
    }

    #[test]
    fn default_tier_picks_a_loadable_file() {
        // With no explicit path and (in CI/local) no reachable Enterprise share,
        // BestAvailable must fall back to the ASN file, which is the small,
        // current, reliable default from the data repository. The data
        // repository is in the cxx submodule, so this resolves when the test
        // runs inside the workspace.
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(IPI_PATH_ENV_VAR);

        if let Some(path) = ipi_data_path(IpiTier::BestAvailable) {
            let chosen = path.to_string_lossy();
            // It is either the Enterprise file (on-network) or the ASN file.
            assert!(
                chosen.contains(IPI_ENTERPRISE_FILE_NAME)
                    || chosen.contains("51Degrees-IPIV4AsnIpiV41.ipi"),
                "unexpected BestAvailable selection: {chosen}"
            );
        }
        // If neither file is present in this checkout the resolver returns None,
        // which is also acceptable; the assertion above is the meaningful one.
    }

    #[test]
    fn default_tier_is_best_available() {
        assert_eq!(IpiTier::default(), IpiTier::BestAvailable);
    }

    #[test]
    fn unreachable_enterprise_share_is_none() {
        // With no explicit IPI path and the Enterprise share variable unset, a
        // forced Enterprise tier yields None rather than an error.
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(IPI_PATH_ENV_VAR);
        std::env::remove_var(IPI_ENTERPRISE_SHARE_ENV_VAR);
        assert_eq!(ipi_data_path(IpiTier::Enterprise), None);
    }

    /// A stand-in for the data repository's shell script. It takes the
    /// arguments the real one does and writes the named file beside its link,
    /// holding the source it was sent with.
    const SHELL_SCRIPT: &str = "while getopts s: opt; do\n\
        case $opt in s) source=$OPTARG ;; esac\n\
        done\n\
        shift $((OPTIND - 1))\n\
        printf '%s' \"$source\" > \"$(dirname \"$0\")/device-detection/$1\"\n";

    /// The PowerShell stand-in, doing what [`SHELL_SCRIPT`] does.
    const POWERSHELL_SCRIPT: &str =
        "param([Parameter(Position = 0)][string]$Name, [string]$Source)\n\
        Set-Content -NoNewline -Value $Source \
        -LiteralPath (Join-Path $PSScriptRoot \"device-detection/$Name\")\n";

    /// A stand-in for either script that fails without writing anything, as
    /// the real scripts do when a download fails.
    const FAILING_SCRIPT: &str = "exit 1\n";

    /// Lay out `<root>/cxx/data` with a link for `test.dat` in its
    /// `device-detection` folder and the two scripts, holding `shell` and
    /// `powershell`, and return a folder below the root to search from.
    fn data_repository(root: &Path, shell: &str, powershell: &str) -> PathBuf {
        let data = root.join("cxx").join("data");
        let folder = data.join("device-detection");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("test.dat.link"),
            "https://example.invalid/test.dat\n",
        )
        .unwrap();
        fs::write(data.join("get-data-files.sh"), shell).unwrap();
        fs::write(data.join("get-data-files.ps1"), powershell).unwrap();
        let start = root.join("examples").join("crate");
        fs::create_dir_all(&start).unwrap();
        start
    }

    const TEST_RELATIVE_PATH: &str = "cxx/data/device-detection/test.dat";

    #[test]
    fn a_missing_linked_file_is_downloaded_by_the_script() {
        let root = tempfile::tempdir().unwrap();
        let start = data_repository(root.path(), SHELL_SCRIPT, POWERSHELL_SCRIPT);

        let found = find_data_file_for(TEST_RELATIVE_PATH, &start, Host::current())
            .expect("the script should have written the missing file");
        assert!(found.ends_with("test.dat"));
        assert_eq!(fs::read_to_string(&found).unwrap(), DATA_DOWNLOAD_SOURCE);
    }

    #[test]
    fn a_file_the_script_does_not_download_is_none() {
        let root = tempfile::tempdir().unwrap();
        let start = data_repository(root.path(), FAILING_SCRIPT, FAILING_SCRIPT);

        assert_eq!(find_data_file_from(TEST_RELATIVE_PATH, &start), None);
    }

    #[test]
    fn a_file_with_no_link_is_none() {
        let root = tempfile::tempdir().unwrap();
        let start = root.path().join("examples");
        fs::create_dir_all(&start).unwrap();

        assert_eq!(find_data_file_from(TEST_RELATIVE_PATH, &start), None);
    }

    #[test]
    fn a_file_that_is_there_is_found_without_running_the_script() {
        let root = tempfile::tempdir().unwrap();
        // Scripts that would overwrite the file, so its content shows they
        // did not run.
        let start = data_repository(root.path(), SHELL_SCRIPT, POWERSHELL_SCRIPT);
        fs::write(root.path().join(TEST_RELATIVE_PATH), b"data").unwrap();

        let found = find_data_file_from(TEST_RELATIVE_PATH, &start).unwrap();
        assert_eq!(fs::read(found).unwrap(), b"data");
    }

    #[test]
    fn the_command_quotes_paths_and_names_with_spaces() {
        let host = Host {
            windows: cfg!(windows),
            has_pwsh: true,
        };
        let command =
            DownloadCommand::for_host(Path::new("cxx/data"), "20000 User Agents.csv", host);
        let expected = if cfg!(windows) {
            "pwsh -NoProfile -ExecutionPolicy Bypass -File \
             \"cxx\\data\\get-data-files.ps1\" \"20000 User Agents.csv\" \
             -Source examples-51degrees-rust"
        } else {
            "sh \"cxx/data/get-data-files.sh\" -s examples-51degrees-rust \
             \"20000 User Agents.csv\""
        };
        assert_eq!(command.to_string(), expected);
    }

    /// The command for a host, which needs neither the network nor the
    /// program it names.
    fn command_for(windows: bool, has_pwsh: bool) -> DownloadCommand {
        let host = Host { windows, has_pwsh };
        DownloadCommand::for_host(Path::new("data"), "file.dat", host)
    }

    #[test]
    fn windows_runs_pwsh_when_it_is_on_the_path() {
        let command = command_for(true, true);
        assert_eq!(command.program, "pwsh");
        assert!(command.removed.is_empty());
    }

    #[test]
    fn windows_powershell_runs_without_the_powershell_7_module_path() {
        let command = command_for(true, false);
        assert_eq!(command.program, "powershell");
        assert_eq!(command.removed, vec!["PSModulePath"]);
        assert!(command
            .to_string()
            .ends_with("(with PSModulePath removed from its environment)"));
    }

    #[test]
    fn other_systems_run_the_shell_script() {
        let command = command_for(false, true);
        assert_eq!(command.program, "sh");
        assert!(command.removed.is_empty());
    }

    #[test]
    fn command_paths_drop_the_verbatim_prefix_on_windows() {
        let path = Path::new(r"\\?\C:\work\cxx\data");
        if cfg!(windows) {
            assert_eq!(command_path(path), r"C:\work\cxx\data");
            assert_eq!(
                command_path(Path::new(r"\\?\UNC\server\share\data")),
                r"\\server\share\data"
            );
        } else {
            assert_eq!(command_path(path), path.display().to_string());
        }
    }
}
