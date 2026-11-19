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

//! ```text
//! parity-compare --corpus <json>... --dotnet <jsonl>... --rust <jsonl>...
//!     --allowed <json> --data <name> [--report <md>] [--strict]
//! ```
//!
//! `--corpus`, `--dotnet` and `--rust` may each be given more than once, and
//! the cases and lines of all the files are compared together. A case id may
//! appear once across the corpus files, and once in each side's lines.
//!
//! Exits with 1 when a difference is not allowed, and with `--strict` also
//! when an entry for these data files allowed nothing, so the list stays
//! true as other changes remove differences.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use parity_compare::{check, compare_all, repeated_id, report, Allowed, Case};

struct Args {
    corpus: Vec<PathBuf>,
    dotnet: Vec<PathBuf>,
    rust: Vec<PathBuf>,
    allowed: PathBuf,
    data: String,
    report: Option<PathBuf>,
    strict: bool,
}

fn main() -> Result<ExitCode> {
    let args = parse_args()?;
    let mut cases: Vec<Case> = Vec::new();
    for path in &args.corpus {
        let list: Vec<Case> = serde_json::from_str(&read(path)?)
            .with_context(|| format!("parsing {}", path.display()))?;
        cases.extend(list);
    }
    if let Some(id) = repeated_id(&cases) {
        bail!("the corpus has two cases with the id {id}");
    }
    let allowed: Vec<Allowed> = serde_json::from_str(&read(&args.allowed)?)
        .with_context(|| format!("parsing {}", args.allowed.display()))?;
    if let Some(problem) = allowed.iter().find_map(Allowed::problem) {
        bail!("{}: {problem}", args.allowed.display());
    }
    let dotnet = lines(&args.dotnet)?;
    let rust = lines(&args.rust)?;

    let differences = compare_all(&cases, &dotnet, &rust);
    let outcome = check(&differences, &allowed, &args.data);
    let text = report(&cases, &differences, &allowed, &outcome, &args.data);
    println!("{text}");
    if let Some(path) = &args.report {
        std::fs::write(path, &text).with_context(|| format!("writing {}", path.display()))?;
    }

    let failed = !outcome.failures.is_empty() || (args.strict && !outcome.unused.is_empty());
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// The lines of one producer's output files, by case id.
fn lines(paths: &[PathBuf]) -> Result<HashMap<String, Value>> {
    let mut out = HashMap::new();
    for path in paths {
        for (number, text) in read(path)?.lines().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let line: Value = serde_json::from_str(text)
                .with_context(|| format!("{} line {}", path.display(), number + 1))?;
            let id = line["id"]
                .as_str()
                .with_context(|| format!("{} line {} has no id", path.display(), number + 1))?
                .to_owned();
            if out.insert(id.clone(), line).is_some() {
                bail!(
                    "{} line {} is a second line for {id}",
                    path.display(),
                    number + 1
                );
            }
        }
    }
    Ok(out)
}

fn parse_args() -> Result<Args> {
    let mut corpus = Vec::new();
    let mut dotnet = Vec::new();
    let mut rust = Vec::new();
    let mut allowed = None;
    let mut data = None;
    let mut report = None;
    let mut strict = false;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--strict" {
            strict = true;
            continue;
        }
        let value = args
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--corpus" => corpus.push(PathBuf::from(value)),
            "--dotnet" => dotnet.push(PathBuf::from(value)),
            "--rust" => rust.push(PathBuf::from(value)),
            "--allowed" => allowed = Some(PathBuf::from(value)),
            "--data" => data = Some(value),
            "--report" => report = Some(PathBuf::from(value)),
            _ => bail!("unknown argument {flag}"),
        }
    }
    for (flag, given) in [
        ("--corpus", &corpus),
        ("--dotnet", &dotnet),
        ("--rust", &rust),
    ] {
        if given.is_empty() {
            bail!("{flag} is required");
        }
    }
    Ok(Args {
        corpus,
        dotnet,
        rust,
        allowed: allowed.context("--allowed is required")?,
        data: data.context("--data is required")?,
        report,
        strict,
    })
}
