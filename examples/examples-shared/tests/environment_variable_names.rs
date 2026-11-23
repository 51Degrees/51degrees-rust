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

//! Every 51Degrees environment variable name in this repository starts with
//! `_51DEGREES_`. The underscore is there because a POSIX shell cannot set a
//! name that starts with a digit, and the number is written as digits rather
//! than spelled out in words because the shorter form is preferred.
//!
//! The tests read every file git tracks, which leaves out build output and
//! the submodules. They fail on any name that starts with the digit, on the
//! number spelled out where an environment variable is named, and on any name
//! a file reads that the repository's own list, `environment-variables.md`,
//! does not hold. A name that is read is a 51Degrees name on that list, or a
//! name another tool defines, which the list holds under its last heading.
//!
//! The last tests read how each workflow hands secrets to the workflows and
//! steps it shares with other repositories. A job that calls a shared workflow
//! passes `secrets: inherit`, and a shared step is given no secret as an input.
//! The shared step that fetches data files keeps only free files in the
//! Actions cache, and fetches each file that needs the license key without
//! it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The start of a name that a shell cannot set, split so this file does not
/// match itself.
const DIGIT_INITIAL: &str = concat!("5", "1DEGREES_");

/// The start of a name with the number spelled out in words, split so this
/// file does not match itself.
const SPELLED_OUT: &str = concat!("FIFTY", "ONE_");

/// The start of the C library's own names, such as the definitions it is
/// compiled with, which are not environment variables.
const C_LIBRARY: &str = concat!("FIFTY", "ONE_DEGREES_");

/// The start of every name 51Degrees defines.
const OURS: &str = "_51DEGREES_";

/// The repository's list of the names it reads, relative to its root.
const LIST: &str = "environment-variables.md";

/// The heading of the list's table of names other tools define.
const OTHER_TOOLS_HEADING: &str = "## Names other tools define";

/// The heading of the list's table of the secrets and variables only CI
/// uses, which have no `_51DEGREES_` start because no customer sets them.
const CI_SECRETS_HEADING: &str = "## Secrets and variables only CI uses";

/// The heading of the list's table of the names a person sets, which have no
/// organization secret.
const PERSON_SETS_HEADING: &str = "## Names a person sets";

/// This file, relative to the repository root. Its own tests write names
/// that nothing reads.
const THIS_FILE: &str = "examples/examples-shared/tests/environment_variable_names.rs";

/// The bytes after which a name in code, a script or a document is an
/// environment variable's, as in a string, a code span, `NAME=value` or
/// `$NAME`.
const NAME_BEFORE: &[u8] = b"\"'`=$";

/// The text after which a name is an environment variable's in PowerShell and
/// in a workflow expression, as in `$env:NAME` and `env.NAME`.
const ENV_BEFORE: [&str; 2] = ["env:", "env."];

/// The text straight before a name that Rust code reads or sets, where the
/// name is a string literal that follows, with white space allowed between.
const RUST_BEFORE_LITERAL: [&str; 8] = [
    "env::var(",
    "env::var_os(",
    "env!(",
    "option_env!(",
    "set_var(",
    "remove_var(",
    ".env(",
    ".env_remove(",
];

/// The text straight before a name a build script sets or watches.
const BUILD_SCRIPT_BEFORE: [&str; 2] = ["rustc-env=", "rerun-if-env-changed="];

/// The text straight before a name that PowerShell or a workflow expression
/// reads, whatever the case of the first letter.
const SCRIPT_BEFORE: [&str; 4] = ["env:", "env.", "secrets.", "vars."];

/// The repository root, two levels above this crate.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The files git tracks, as paths relative to `root`.
fn tracked_files(root: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git ls-files failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect()
}

/// Each tracked file that is a file here, with its bytes. A submodule is
/// tracked as a directory, and its files belong to another repository.
fn tracked_file_contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    tracked_files(root)
        .into_iter()
        .filter(|relative| root.join(relative).is_file())
        .map(|relative| {
            let bytes = std::fs::read(root.join(&relative))
                .unwrap_or_else(|error| panic!("read {relative}: {error}"));
            (relative, bytes)
        })
        .collect()
}

/// A byte that can be part of an environment variable name.
fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The name that starts at `at`, which runs to the first byte that cannot be
/// part of one.
fn name_at(line: &[u8], at: usize) -> String {
    let end = line[at..]
        .iter()
        .position(|byte| !is_name_byte(*byte))
        .map_or(line.len(), |length| at + length);
    String::from_utf8_lossy(&line[at..end]).into_owned()
}

/// Each name in `line` that starts with [`DIGIT_INITIAL`] and is not the end
/// of a longer name, such as one with a leading underscore.
fn digit_initial_names(line: &[u8]) -> Vec<String> {
    let needle = DIGIT_INITIAL.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < line.len() {
        if line[at..].starts_with(needle) && (at == 0 || !is_name_byte(line[at - 1])) {
            let name = name_at(line, at);
            at += name.len();
            names.push(name);
        } else {
            at += 1;
        }
    }
    names
}

/// True for a file in which every name is an environment variable's, being
/// a workflow, whose `env` keys are names.
fn is_workflow(relative: &str) -> bool {
    relative.ends_with(".yml") || relative.ends_with(".yaml")
}

/// True when `relative` is a test file of an example crate, which the
/// examples workflow runs with the names only CI uses.
fn is_test(relative: &str) -> bool {
    relative.starts_with("examples/") && relative.contains("/tests/") && relative.ends_with(".rs")
}

/// Each name in `line` that starts with [`SPELLED_OUT`], is not the end of a
/// longer name or one of [`C_LIBRARY`]'s, and is written where an environment
/// variable is named. In a workflow that is anywhere, and elsewhere it is
/// straight after one of [`NAME_BEFORE`] or [`ENV_BEFORE`].
fn spelled_out_names(line: &[u8], workflow: bool) -> Vec<String> {
    let needle = SPELLED_OUT.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < line.len() {
        if !line[at..].starts_with(needle) || (at > 0 && is_name_byte(line[at - 1])) {
            at += 1;
            continue;
        }
        let name = name_at(line, at);
        let before = &line[..at];
        let named_as_variable = workflow
            || before.last().is_some_and(|byte| NAME_BEFORE.contains(byte))
            || ENV_BEFORE
                .iter()
                .any(|text| before.ends_with(text.as_bytes()));
        at += name.len();
        if named_as_variable && !name.starts_with(C_LIBRARY) {
            names.push(name);
        }
    }
    names
}

/// Each 51Degrees name in `line`, being [`OURS`] and at least one more name
/// byte, that is not the end of a longer name.
fn our_names(line: &[u8]) -> Vec<String> {
    let needle = OURS.as_bytes();
    let mut names = Vec::new();
    let mut at = 0;
    while at < line.len() {
        if line[at..].starts_with(needle) && (at == 0 || !is_name_byte(line[at - 1])) {
            let name = name_at(line, at);
            at += name.len();
            if name.len() > OURS.len() {
                names.push(name);
            }
        } else {
            at += 1;
        }
    }
    names
}

/// The names `line` of a Rust file reads or sets: the string literal after
/// one of [`RUST_BEFORE_LITERAL`], the name after one of
/// [`BUILD_SCRIPT_BEFORE`], and the string a constant whose own name says it
/// holds an environment variable's name is given.
fn rust_names_read(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    for before in RUST_BEFORE_LITERAL {
        for (at, _) in line.match_indices(before) {
            // `env!(` is also the end of `option_env!(`, which is found on
            // its own.
            if !before.starts_with('.') && at > 0 && is_name_byte(line.as_bytes()[at - 1]) {
                continue;
            }
            let rest = line[at + before.len()..].trim_start();
            if let Some(literal) = rest.strip_prefix('"') {
                let name = name_at(literal.as_bytes(), 0);
                if !name.is_empty() && literal[name.len()..].starts_with('"') {
                    names.push(name);
                }
            }
        }
    }
    for before in BUILD_SCRIPT_BEFORE {
        for (at, _) in line.match_indices(before) {
            let name = name_at(line.as_bytes(), at + before.len());
            if !name.is_empty() {
                names.push(name);
            }
        }
    }
    if let Some((declared, value)) = line.split_once(": &str = \"") {
        let holds_a_name = declared.trim_start().starts_with("const ")
            || declared.trim_start().starts_with("pub const ");
        let says_so = ["ENV_VAR", "_VARIABLE", "_ENV"]
            .iter()
            .any(|part| declared.contains(part));
        let name = name_at(value.as_bytes(), 0);
        if holds_a_name && says_so && !name.is_empty() && value[name.len()..].starts_with('"') {
            names.push(name);
        }
    }
    names
}

/// The names `line` of a script, a workflow or a document reads: the name
/// after one of [`SCRIPT_BEFORE`], where that is not the end of a longer
/// word.
fn script_names_read(line: &str) -> Vec<String> {
    let lower = line.to_ascii_lowercase();
    let mut names = Vec::new();
    for before in SCRIPT_BEFORE {
        for (at, _) in lower.match_indices(before) {
            if at > 0 && is_name_byte(line.as_bytes()[at - 1]) {
                continue;
            }
            let name = name_at(line.as_bytes(), at + before.len());
            // A name is written in capitals, or as Windows writes its own.
            // Lower case text here is a field of an expression, such as
            // `env.name` in prose.
            if name
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
            {
                names.push(name);
            }
        }
    }
    names
}

/// The name a workflow line sets as the key of an `env` block, which is
/// written in capitals where an input or a step's own key is not.
fn workflow_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let name = name_at(trimmed.as_bytes(), 0);
    let is_key = trimmed[name.len()..].starts_with(':')
        && name.len() > 1
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name.chars().any(|c| c.is_ascii_uppercase());
    is_key.then_some(name)
}

/// The name a workflow line writes for the steps after it, as in
/// `echo "NAME=value" >> "$GITHUB_ENV"`.
fn workflow_written(line: &str) -> Option<String> {
    if !line.contains("GITHUB_ENV") {
        return None;
    }
    let (_, rest) = line.split_once("echo \"")?;
    let name = name_at(rest.as_bytes(), 0);
    (!name.is_empty() && rest[name.len()..].starts_with('=')).then_some(name)
}

/// The names the repository's list holds, read from the first cell of each
/// table row.
struct Listed {
    /// The 51Degrees names.
    ours: BTreeSet<String>,
    /// The names other tools define.
    other_tools: BTreeSet<String>,
    /// The secrets and variables only CI uses.
    ci_secrets: BTreeSet<String>,
    /// The names a person sets.
    person_sets: BTreeSet<String>,
    /// The starts of the names other tools define, each written in the list
    /// with a `*` after it.
    other_tool_prefixes: Vec<String>,
}

impl Listed {
    fn read(text: &str) -> Self {
        let mut listed = Listed {
            ours: BTreeSet::new(),
            other_tools: BTreeSet::new(),
            ci_secrets: BTreeSet::new(),
            person_sets: BTreeSet::new(),
            other_tool_prefixes: Vec::new(),
        };
        let mut other_tools = false;
        let mut ci_secrets = false;
        let mut person_sets = false;
        for line in text.lines() {
            if line.starts_with("## ") {
                other_tools = line.trim_end() == OTHER_TOOLS_HEADING;
                ci_secrets = line.trim_end() == CI_SECRETS_HEADING;
                person_sets = line.trim_end() == PERSON_SETS_HEADING;
                continue;
            }
            let Some(cell) = line.strip_prefix("| `") else {
                continue;
            };
            let Some((name, _)) = cell.split_once('`') else {
                continue;
            };
            if ci_secrets {
                listed.ci_secrets.insert(name.to_owned());
                continue;
            }
            if person_sets {
                listed.person_sets.insert(name.to_owned());
            }
            match (other_tools, name.strip_suffix('*')) {
                (true, Some(prefix)) => listed.other_tool_prefixes.push(prefix.to_owned()),
                (true, None) => {
                    listed.other_tools.insert(name.to_owned());
                }
                (false, _) => {
                    listed.ours.insert(name.to_owned());
                }
            }
        }
        listed
    }

    /// True when the list allows `name` to be read.
    fn allows(&self, name: &str) -> bool {
        self.ours.contains(name)
            || self.other_tools.contains(name)
            || self
                .other_tool_prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix.as_str()))
    }
}

/// The repository's list.
fn listed(root: &Path) -> Listed {
    let text = std::fs::read_to_string(root.join(LIST))
        .unwrap_or_else(|error| panic!("read {LIST}: {error}"));
    Listed::read(&text)
}

#[test]
fn the_spelled_out_matcher_finds_only_environment_variable_names() {
    let variable = format!("{SPELLED_OUT}CXX_DIR");
    let constant = format!("{SPELLED_OUT}PREFIX");
    let c_library = format!("{C_LIBRARY}REDUCED_FILE");
    let line = format!(
        "var(\"{variable}\") `{variable}` rustc-env={variable} $env:{variable} \
         constants::{constant} (\"{c_library}\") EVIDENCE_{constant}"
    );
    assert_eq!(spelled_out_names(line.as_bytes(), false), vec![variable; 4]);
    let key = format!("  {SPELLED_OUT}KEY: value");
    assert_eq!(
        spelled_out_names(key.as_bytes(), true),
        vec![format!("{SPELLED_OUT}KEY")]
    );
}

#[test]
fn no_file_names_a_variable_with_the_number_spelled_out() {
    let root = repository_root();
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        let workflow = is_workflow(&relative);
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            for name in spelled_out_names(line, workflow) {
                found.push(format!("{relative}:{}: {name}", index + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} environment variable names spell the number out, where the \
         shorter _51DEGREES_ form is preferred:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_name_matcher_finds_only_names_that_start_with_the_digit() {
    let digit_initial = format!("{DIGIT_INITIAL}DD_PATH");
    let line = format!("export {digit_initial}=a _{digit_initial}=b X{digit_initial}");
    assert_eq!(digit_initial_names(line.as_bytes()), vec![digit_initial]);
}

#[test]
fn no_file_names_a_variable_that_starts_with_a_digit() {
    let root = repository_root();
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
            for name in digit_initial_names(line) {
                found.push(format!("{relative}:{}: {name}", index + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} names start with a digit, which a POSIX shell cannot set, so \
         each must start with an underscore:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_matchers_find_the_names_a_line_reads() {
    assert_eq!(
        our_names(b"`_51DEGREES_DD_PATH`, X_51DEGREES_NOT and _51DEGREES_<NAME> _51DEGREES_A*"),
        vec!["_51DEGREES_DD_PATH", "_51DEGREES_A"]
    );
    assert_eq!(
        rust_names_read(
            "std::env::var(\"ONE\"); env::var_os( \"TWO\"); option_env!(\"THREE\"); \
             command.env(\"FOUR\", value).env_remove(\"FIVE\"); env::var(name); \
             println!(\"cargo:rustc-env=SIX={}\", value); env::var(\"not a name\")"
        ),
        vec!["ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX"]
    );
    assert_eq!(
        rust_names_read("pub const DD_PATH_ENV_VAR: &str = \"SEVEN\";"),
        vec!["SEVEN"]
    );
    assert!(rust_names_read("const TITLE: &str = \"EIGHT\";").is_empty());
    assert_eq!(
        script_names_read(
            "$env:ONE ${env:TWO} Env:THREE ${{ secrets.FOUR }} vars.FIVE env.SIX \
             $env:ProgramFiles steps.keys.outputs.run github.event_name"
        ),
        vec!["ONE", "TWO", "THREE", "ProgramFiles", "SIX", "FOUR", "FIVE"]
    );
    assert_eq!(
        workflow_key("          SEVEN: value"),
        Some("SEVEN".to_owned())
    );
    assert_eq!(
        workflow_key("          _51DEGREES_EIGHT: |"),
        Some("_51DEGREES_EIGHT".to_owned())
    );
    assert_eq!(workflow_key("        shell: pwsh"), None);
    assert_eq!(
        workflow_written("  echo \"_51DEGREES_NINE=true\" >> \"$GITHUB_ENV\""),
        Some("_51DEGREES_NINE".to_owned())
    );
    assert_eq!(workflow_written("  echo \"TEN=true\""), None);
    assert_eq!(workflow_key("      - name: Checkout"), None);
}

#[test]
fn the_list_is_read_by_its_headings() {
    let listed = Listed::read(
        "## Names a person sets\n\
         | Name | What it holds |\n\
         | --- | --- |\n\
         | `_51DEGREES_ONE` | One thing. |\n\
         ## Names other tools define\n\
         | `TOOL_*` | A tool |\n\
         | `PATH` | The operating system |\n",
    );
    assert!(listed.allows("_51DEGREES_ONE"));
    assert!(listed.allows("TOOL_HOME"));
    assert!(listed.allows("PATH"));
    assert!(!listed.allows("_51DEGREES_TWO"));
    assert!(!listed.allows("PATHS"));
    assert!(!listed.allows("OTHER"));
}

#[test]
fn every_51degrees_name_on_the_list_starts_as_they_all_do_and_is_read() {
    let root = repository_root();
    let listed = listed(&root);
    assert!(!listed.ours.is_empty(), "{LIST} lists no names");
    let wrong: Vec<&String> = listed
        .ours
        .iter()
        .filter(|name| !name.starts_with(OURS))
        .collect();
    assert!(
        wrong.is_empty(),
        "{LIST} lists these as 51Degrees names, and each must start {OURS}: {wrong:?}"
    );
    // A name nothing reads any longer comes off the list.
    let mut unread = listed.ours.clone();
    for (relative, bytes) in tracked_file_contents(&root) {
        if relative == LIST || relative == THIS_FILE {
            continue;
        }
        for line in bytes.split(|byte| *byte == b'\n') {
            for name in our_names(line) {
                unread.remove(&name);
            }
        }
    }
    assert!(
        unread.is_empty(),
        "{LIST} lists names that no file reads: {unread:?}"
    );
}

#[test]
fn every_name_a_file_reads_is_on_the_list() {
    let root = repository_root();
    let listed = listed(&root);
    let mut found = BTreeSet::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        if relative == LIST || relative == THIS_FILE {
            continue;
        }
        let workflow = is_workflow(&relative);
        let rust = relative.ends_with(".rs");
        let text = String::from_utf8_lossy(&bytes);
        // A variable one workflow sets for its own steps, as a key or by
        // writing it for the steps after, is local to it, so it needs the
        // start every 51Degrees name has and no row in the list.
        let local: BTreeSet<String> = if workflow {
            text.lines()
                .flat_map(|line| workflow_key(line).into_iter().chain(workflow_written(line)))
                .filter(|name| name.starts_with(OURS))
                .collect()
        } else {
            BTreeSet::new()
        };
        for (index, line) in text.lines().enumerate() {
            let mut names = our_names(line.as_bytes());
            if rust {
                names.extend(rust_names_read(line));
            } else {
                names.extend(script_names_read(line));
            }
            if workflow {
                names.extend(workflow_key(line));
            }
            for name in names {
                // A name only CI uses is read by a workflow, or by a test the
                // workflow runs, and nothing else.
                let ci_secret =
                    (workflow || is_test(&relative)) && listed.ci_secrets.contains(&name);
                if !listed.allows(&name) && !ci_secret && !local.contains(&name) {
                    found.insert(format!("{relative}:{}: {name}", index + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} names are read that {LIST} does not hold. A name 51Degrees \
         defines starts {OURS} and has a row there, and a name another tool \
         defines has a row under \"{OTHER_TOOLS_HEADING}\":\n{}",
        found.len(),
        found.into_iter().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn every_ci_name_on_the_list_has_no_start_and_is_read_by_a_workflow() {
    let root = repository_root();
    let listed = listed(&root);
    let prefixed: Vec<&String> = listed
        .ci_secrets
        .iter()
        .filter(|name| name.starts_with(OURS))
        .collect();
    assert!(
        prefixed.is_empty(),
        "{LIST} lists these under \"{CI_SECRETS_HEADING}\", and none may start {OURS}: {prefixed:?}"
    );
    // A name no workflow reads any longer comes off the list.
    let mut unread = listed.ci_secrets.clone();
    for (relative, bytes) in tracked_file_contents(&root) {
        if !is_workflow(&relative) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let read = secrets_read(&text);
        let vars = context_read(&text, "vars.");
        unread.retain(|name| !read.contains(name) && !vars.contains(name));
    }
    assert!(
        unread.is_empty(),
        "{LIST} lists secrets or variables that no workflow reads: {unread:?}"
    );
}

/// The names a workflow reads from the secrets context, as `secrets.NAME`.
fn secrets_read(text: &str) -> BTreeSet<String> {
    context_read(text, "secrets.")
}

/// The names a workflow reads from a context, such as `vars.NAME` for the
/// context `vars.`.
fn context_read(text: &str, context: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (at, _) in text.match_indices(context) {
        let name: String = text[at + context.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            names.insert(name);
        }
    }
    names
}

#[test]
fn the_secrets_matcher_finds_each_name_however_it_is_spaced() {
    let read = secrets_read("a: ${{secrets.ONE}}
b: ${{ secrets.TWO || '' }}
c: steps.secrets.outputs");
    assert!(read.contains("ONE") && read.contains("TWO"), "{read:?}");
    let vars = context_read("d: ${{ vars.THREE }}", "vars.");
    assert!(vars.contains("THREE"), "{vars:?}");
}

#[test]
fn only_a_workflow_or_an_example_test_may_read_a_name_only_ci_uses() {
    assert!(is_test("examples/one/tests/live.rs"));
    assert!(!is_test("tests/live.rs"));
    assert!(!is_test("fodid-client/tests/live_cloud.rs"));
    assert!(!is_test("examples/one/src/lib.rs"));
    assert!(!is_test("examples/one/tests/fixture.json"));
}

/// A name a person sets has no organization secret, so a workflow sets it
/// from a secret listed under "Secrets and variables only CI uses" rather
/// than reading a secret of the same name.
#[test]
fn no_workflow_reads_a_name_a_person_sets_from_the_secrets() {
    let root = repository_root();
    let listed = listed(&root);
    assert!(
        !listed.person_sets.is_empty(),
        "{LIST} has no names under \"{PERSON_SETS_HEADING}\""
    );
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        if !is_workflow(&relative) {
            continue;
        }
        for name in secrets_read(&String::from_utf8_lossy(&bytes)) {
            if listed.person_sets.contains(&name) {
                found.push(format!("{relative}: secrets.{name}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "these workflows read a name a person sets from the secrets: {found:?}"
    );
}

/// The start of every Resource Key's name. Each Resource Key is an
/// organization secret, and never a variable.
const RESOURCE_KEY_PREFIX: &str = "_51DEGREES_RESOURCE_KEY";

/// The names a workflow reads from the organization's variables, the cloud
/// and the staging cloud. Every key is a secret.
const VARIABLES: [&str; 2] = ["_51DEGREES_CLOUD_ENDPOINT", "CLOUD_STAGING_ENDPOINT"];

#[test]
fn a_workflow_reads_the_resource_keys_from_the_secrets() {
    let root = repository_root();
    let before = "vars.";
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        if !is_workflow(&relative) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.lines().enumerate() {
            for (at, _) in line.match_indices(before) {
                if at > 0 && is_name_byte(line.as_bytes()[at - 1]) {
                    continue;
                }
                let name = name_at(line.as_bytes(), at + before.len());
                if name.starts_with(RESOURCE_KEY_PREFIX) {
                    found.push(format!("{relative}:{}: {name}", index + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} Resource Keys are read from the variables, where they are secrets:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn a_workflow_reads_only_the_listed_names_from_the_variables() {
    let root = repository_root();
    let before = "vars.";
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        if !is_workflow(&relative) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (index, line) in text.lines().enumerate() {
            for (at, _) in line.match_indices(before) {
                if at > 0 && is_name_byte(line.as_bytes()[at - 1]) {
                    continue;
                }
                let name = name_at(line.as_bytes(), at + before.len());
                if !VARIABLES.contains(&name.as_str()) {
                    found.push(format!("{relative}:{}: {name}", index + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} names are read from the variables that are not among {}:\n{}",
        found.len(),
        VARIABLES.join(", "),
        found.join("\n")
    );
}

/// The start of the address of a workflow or a step shared between
/// repositories.
const SHARED: &str = "51Degrees/common-ci/";

/// The part of an address that makes it a shared workflow, which a job
/// calls, rather than a shared step.
const SHARED_WORKFLOW: &str = "/.github/workflows/";

/// The number of spaces `line` starts with.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// True for a line that belongs to whatever block is around it, being
/// empty or a comment.
fn is_filler(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('#')
}

/// The lines around line `at` that are indented by `least` or more, which
/// are the lines of the job or the step `at` is in. A step's first line is
/// taken as indented to its key, after the `- `.
fn block_around(lines: &[&str], at: usize, least: usize) -> Vec<String> {
    let key_line = |line: &str| -> Option<String> {
        let spaces = indent(line);
        let rest = &line[spaces..];
        match rest.strip_prefix("- ") {
            Some(first) if spaces + 2 == least => Some(format!("{}{first}", " ".repeat(least))),
            _ if spaces >= least => Some(line.to_owned()),
            _ => None,
        }
    };
    let mut start = at;
    let first_of_a_step =
        |line: &str| line.trim_start().starts_with("- ") && indent(line) + 2 == least;
    while start > 0 && !first_of_a_step(lines[start]) {
        let before = lines[start - 1];
        if !is_filler(before) && key_line(before).is_none() {
            break;
        }
        start -= 1;
    }
    let mut block = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(start) {
        if is_filler(line) {
            continue;
        }
        let starts_a_step = index > start && first_of_a_step(line);
        match key_line(line) {
            Some(line) if !starts_a_step => block.push(line),
            _ => break,
        }
    }
    block
}

/// Each place where the workflow `text` hands a secret to a shared workflow
/// or a shared step in a way other than the one way, as the number of the
/// line that calls it and what is wrong. A job that calls a shared workflow
/// passes `secrets: inherit` and names no secret. A shared step reads its
/// keys from the environment, so it is given no secret as an input.
fn shared_call_faults(text: &str) -> Vec<(usize, &'static str)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut faults = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let spaces = indent(line);
        let rest = &line[spaces..];
        let (key, least) = match rest.strip_prefix("- ") {
            Some(key) => (key, spaces + 2),
            None => (rest, spaces),
        };
        let Some(address) = key.strip_prefix("uses:") else {
            continue;
        };
        if !address.trim_start().starts_with(SHARED) {
            continue;
        }
        let block = block_around(&lines, at, least);
        let keys = || block.iter().filter(|line| indent(line) == least);
        if address.contains(SHARED_WORKFLOW) {
            let secrets: Vec<&str> = keys()
                .map(|line| line.trim())
                .filter(|line| line.starts_with("secrets:"))
                .collect();
            match secrets.as_slice() {
                ["secrets: inherit"] => {}
                [] => faults.push((at + 1, "passes no `secrets: inherit`")),
                _ => faults.push((at + 1, "names a secret, where it passes `secrets: inherit`")),
            }
            continue;
        }
        let mut inputs = false;
        for line in &block {
            if indent(line) == least {
                inputs = line.trim().starts_with("with:");
            } else if inputs && line.contains("secrets.") {
                faults.push((at + 1, "is given a secret as an input"));
                break;
            }
        }
    }
    faults
}

#[test]
fn the_shared_call_matcher_finds_each_other_way_of_passing_a_secret() {
    let text = "\
jobs:
  one:
    name: One
    uses: 51Degrees/common-ci/.github/workflows/one.yml@main
    with:
      repo-name: here
    secrets: inherit

  two:
    uses: 51Degrees/common-ci/.github/workflows/two.yml@main
    secrets:
      _51DEGREES_ONE: ${{ secrets._51DEGREES_ONE }}
  three:
    # Needs nothing.
    uses: 51Degrees/common-ci/.github/workflows/three.yml@main
  four:
    runs-on: ubuntu-latest
    steps:
      - name: Own step
        env:
          _51DEGREES_ONE: ${{ secrets._51DEGREES_ONE }}
        run: ./ci/own.ps1
      - name: Shared step
        uses: 51Degrees/common-ci/actions/step@main
        env:
          _51DEGREES_ONE: ${{ secrets._51DEGREES_ONE }}
        with:
          cache: true
      - uses: 51Degrees/common-ci/actions/step@main
        with:
          cache: true
          key: ${{ secrets._51DEGREES_ONE }}
      - name: Another tool's step
        uses: actions/cache@v4
        with:
          key: ${{ secrets._51DEGREES_ONE }}
";
    assert_eq!(
        shared_call_faults(text),
        vec![
            (10, "names a secret, where it passes `secrets: inherit`"),
            (15, "passes no `secrets: inherit`"),
            (29, "is given a secret as an input"),
        ]
    );
}

#[test]
fn every_shared_workflow_inherits_its_secrets() {
    let root = repository_root();
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        if !is_workflow(&relative) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (line, fault) in shared_call_faults(&text) {
            found.push(format!("{relative}:{line}: {fault}"));
        }
    }
    assert!(
        found.is_empty(),
        "{} calls of a shared workflow or step pass a secret another way. A \
         job that calls a shared workflow passes `secrets: inherit` and no \
         secret by name, and a shared step reads its keys from the \
         environment:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// The address of the shared step that fetches data files, before its ref,
/// in lower case because GitHub reads an owner in any case.
const FETCH_ASSETS: &str = "51degrees/common-ci/actions/fetch-assets@";

/// The addresses of the steps that keep their `path` when they run, being
/// the cache's save step and the artifact upload.
const KEEPS_PATH_NOW: [&str; 2] = ["actions/cache/save@", "actions/upload-artifact@"];

/// The address of the cache step that keeps its `path` when the job ends,
/// whatever its place in the job.
const KEEPS_PATH_AT_END: &str = "actions/cache@";

/// The assets the shared fetch step downloads with the data license key, as
/// common-ci's steps/fetch-assets.ps1 lists them. Every bulk data source,
/// named as in [`is_data_source`], needs the key as well.
const LICENSED_ASSETS: [&str; 4] = [
    "TAC-HashV41.hash",
    "51Degrees-EnterpriseIpiV41.ipi",
    "51Degrees.csv",
    "51Degrees-Tac.zip",
];

/// True for an asset that names a bulk data source, being lower case
/// letters, digits, and hyphens before `.json`, with no hyphen first.
fn is_data_source(asset: &str) -> bool {
    asset.strip_suffix(".json").is_some_and(|stem| {
        stem.bytes().next().is_some_and(|byte| byte != b'-')
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

/// True for an asset the shared fetch step downloads with the license key.
fn needs_license(asset: &str) -> bool {
    LICENSED_ASSETS.contains(&asset) || is_data_source(asset)
}

/// `line` without a comment that follows white space.
fn without_comment(line: &str) -> &str {
    match line.find(" #") {
        Some(at) => line[..at].trim_end(),
        None => line,
    }
}

/// `value` without white space and the quotes around it.
fn unquote(value: &str) -> &str {
    value.trim().trim_matches(|c| c == '\'' || c == '"')
}

/// The key and the value of `text`, as `key: value`.
fn key_and_value(text: &str) -> (&str, &str) {
    let (key, value) = text.split_once(':').unwrap_or((text, ""));
    (key.trim(), unquote(value))
}

/// The inputs a step is given under `with:`, each with its values, from the
/// step's lines as [`block_around`] gives them, with its keys indented by
/// `least`. A block value such as `assets: |` gives one value per line.
/// Inputs in flow style, as `with: { assets: one, cache: true }`, are read
/// when they are on the one line.
fn step_inputs(block: &[String], least: usize) -> Vec<(String, Vec<String>)> {
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    let mut inputs = false;
    let mut input_indent = None;
    let mut in_block_value = false;
    for line in block {
        let line = without_comment(line);
        let spaces = indent(line);
        if spaces <= least {
            let (key, value) = key_and_value(line.trim());
            inputs = spaces == least && key == "with";
            input_indent = None;
            in_block_value = false;
            let flow = value.strip_prefix('{').and_then(|v| v.strip_suffix('}'));
            if let (true, Some(flow)) = (inputs, flow) {
                for pair in flow.split(',') {
                    let (key, value) = key_and_value(pair.trim());
                    found.push((key.to_owned(), vec![value.to_owned()]));
                }
                inputs = false;
            }
            continue;
        }
        if !inputs {
            continue;
        }
        let input_indent = *input_indent.get_or_insert(spaces);
        if spaces > input_indent {
            if let (true, Some((_, values))) = (in_block_value, found.last_mut()) {
                values.push(line.trim().to_owned());
            }
            continue;
        }
        let (key, value) = key_and_value(line.trim());
        in_block_value = value.starts_with(['|', '>']);
        let values = if in_block_value || value.is_empty() {
            Vec::new()
        } else {
            vec![value.to_owned()]
        };
        found.push((key.to_owned(), values));
    }
    found
}

/// The values of the input `name` among `inputs`.
fn input_values<'a>(inputs: &'a [(String, Vec<String>)], name: &str) -> Vec<&'a str> {
    inputs
        .iter()
        .filter(|(key, _)| key == name)
        .flat_map(|(_, values)| values.iter().map(String::as_str))
        .collect()
}

/// True when the `path` value `path` covers `asset`, which the shared fetch
/// step writes to the `assets` folder in the workspace. A path that starts
/// with `!` leaves files out, so it covers nothing.
fn path_covers(path: &str, asset: &str) -> bool {
    let path = unquote(path);
    if path.starts_with('!') {
        return false;
    }
    let path = path
        .strip_prefix("${{ github.workspace }}")
        .unwrap_or(path)
        .trim_start_matches('/')
        .trim_start_matches("./")
        .trim_end_matches('/');
    let in_folder = path.strip_prefix("assets/");
    path.is_empty()
        || path == "."
        || path.starts_with('*')
        || path == "assets"
        || in_folder.is_some_and(|file| file == asset || file.contains('*'))
        || path == asset
        || path.ends_with(&format!("/{asset}"))
}

/// Each licensed file among `licensed` that a step keeping `paths` covers,
/// with `line`.
fn kept_files(line: usize, paths: &[String], licensed: &[String]) -> Vec<(usize, String)> {
    licensed
        .iter()
        .filter(|asset| paths.iter().any(|path| path_covers(path, asset)))
        .map(|asset| (line, asset.clone()))
        .collect()
}

/// The licensed files that the cache steps which save when the job ends,
/// `kept_at_end`, keep of `licensed`, the files fetched in the job.
fn kept_at_job_end(
    kept_at_end: &[(usize, Vec<String>)],
    licensed: &[String],
) -> Vec<(usize, String)> {
    kept_at_end
        .iter()
        .flat_map(|(line, paths)| kept_files(*line, paths, licensed))
        .collect()
}

/// Each licensed data file that a step in the workflow `text` keeps in the
/// Actions cache or in an artifact, with the number of the line that calls
/// the step. The shared fetch step keeps its whole folder, so a call that
/// asks for the cache after a licensed file was fetched in the same job
/// keeps that file too. A cache save or an artifact upload after the fetch
/// keeps the file when its `path` covers it, and the cache step that saves
/// when the job ends does so wherever it is in the job.
fn cached_licensed_files(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    let mut in_jobs = false;
    let mut licensed_in_job: Vec<String> = Vec::new();
    let mut kept_at_end: Vec<(usize, Vec<String>)> = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if is_filler(line) {
            continue;
        }
        let line = without_comment(line);
        let spaces = indent(line);
        let rest = &line[spaces..];
        if spaces == 0 || (in_jobs && spaces == 2) {
            if spaces == 0 {
                in_jobs = rest.trim_end() == "jobs:";
            }
            found.extend(kept_at_job_end(&kept_at_end, &licensed_in_job));
            kept_at_end.clear();
            licensed_in_job.clear();
            continue;
        }
        let (key, least) = match rest.strip_prefix("- ") {
            Some(key) => (key, spaces + 2),
            None => (rest, spaces),
        };
        let Some(address) = key.strip_prefix("uses:") else {
            continue;
        };
        let address = unquote(address).to_ascii_lowercase();
        let inputs = step_inputs(&block_around(&lines, at, least), least);
        let paths = || -> Vec<String> {
            input_values(&inputs, "path")
                .into_iter()
                .map(str::to_owned)
                .collect()
        };
        if address.starts_with(FETCH_ASSETS) {
            let licensed: Vec<String> = input_values(&inputs, "assets")
                .into_iter()
                .filter(|asset| needs_license(asset))
                .map(str::to_owned)
                .collect();
            let cache = input_values(&inputs, "cache")
                .iter()
                .any(|value| !value.is_empty() && *value != "false");
            if cache {
                for asset in licensed_in_job.iter().chain(&licensed) {
                    found.push((at + 1, asset.clone()));
                }
            }
            licensed_in_job.extend(licensed);
        } else if KEEPS_PATH_NOW.iter().any(|now| address.starts_with(now)) {
            found.extend(kept_files(at + 1, &paths(), &licensed_in_job));
        } else if address.starts_with(KEEPS_PATH_AT_END) {
            kept_at_end.push((at + 1, paths()));
        }
    }
    found.extend(kept_at_job_end(&kept_at_end, &licensed_in_job));
    found.sort_by_key(|(line, _)| *line);
    found
}

#[test]
fn the_fetch_matcher_finds_each_cached_licensed_file() {
    let text = "\
jobs:
  one:
    steps:
      - name: Free files, cached
        uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: |
            51Degrees-LiteV4.1.hash
            51Degrees-IPIV4AsnIpiV41.ipi
          cache: true
      - name: Licensed files, cached
        uses: 51Degrees/common-ci/actions/fetch-assets@main
        env:
          _51DEGREES_LICENSE_KEY: ${{ secrets.ONE }}
        with:
          assets: |
            TAC-HashV41.hash
            51Degrees-EnterpriseIpiV41.ipi
          # Kept for the day.
          cache: 'true'
  two:
    steps:
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: devices.json
          cache: true
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: 51Degrees.csv
  three:
    steps:
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: 51Degrees-Tac.zip
          cache: false
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: 51Degrees-LiteV4.1.hash
          cache: true
  four:
    steps:
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: 51Degrees-LiteV4.1.hash
          cache: true
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: TAC-HashV41.hash
  five:
    steps:
      - uses: '51Degrees/common-ci/actions/fetch-assets@main'
        with:  # The licensed file.
          assets: TAC-HashV41.hash
          cache: true
      - uses: 51degrees/common-ci/actions/fetch-assets@main
        with: { assets: 51Degrees-EnterpriseIpiV41.ipi, cache: true }
  six:
    steps:
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: TAC-HashV41.hash
      - name: Keep the folder
        uses: actions/cache/save@v4
        with:
          path: assets
          key: one
      - uses: actions/upload-artifact@v4
        with:
          name: files
          path: |
            assets/TAC-HashV41.hash
            parity/out
      - uses: actions/upload-artifact@v4
        with:
          name: reports
          path: parity/out
  seven:
    steps:
      - uses: actions/cache@v4
        with:
          path: ${{ github.workspace }}/assets
          key: two
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: 51Degrees.csv
      - uses: actions/cache/save@v4
        with:
          path: cxx/data
          key: three
  eight:
    steps:
      - uses: actions/upload-artifact@v4
        with:
          path: ./assets
      - uses: 51Degrees/common-ci/actions/fetch-assets@main
        with:
          assets: TAC-HashV41.hash
";
    assert_eq!(
        cached_licensed_files(text),
        vec![
            (12, "TAC-HashV41.hash".to_owned()),
            (12, "51Degrees-EnterpriseIpiV41.ipi".to_owned()),
            (23, "devices.json".to_owned()),
            (36, "51Degrees-Tac.zip".to_owned()),
            (51, "TAC-HashV41.hash".to_owned()),
            (55, "TAC-HashV41.hash".to_owned()),
            (55, "51Degrees-EnterpriseIpiV41.ipi".to_owned()),
            (63, "TAC-HashV41.hash".to_owned()),
            (67, "TAC-HashV41.hash".to_owned()),
            (79, "51Degrees.csv".to_owned()),
        ]
    );
}

#[test]
fn no_workflow_caches_a_licensed_data_file() {
    let root = repository_root();
    let mut found = Vec::new();
    for (relative, bytes) in tracked_file_contents(&root) {
        if !is_workflow(&relative) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (line, asset) in cached_licensed_files(&text) {
            found.push(format!("{relative}:{line}: {asset}"));
        }
    }
    assert!(
        found.is_empty(),
        "{} licensed data files are kept in the Actions cache or an artifact. \
         Fetch each without asking the shared fetch step for the cache, after \
         every step in the same job that does, and keep no path that covers \
         the file in a cache or artifact step of that job:\n{}",
        found.len(),
        found.join("\n")
    );
}
