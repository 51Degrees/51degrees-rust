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

//! On-premise engine: move the star-sign boundaries into a data file the engine
//! reads at start-up and can reload on demand.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValueType, TypedKey,
};
use pipeline_examples::star_sign::{
    parse_day_month, star_sign_for, StarSignBoundary, StarSignData, STAR_SIGNS, STAR_SIGN_DATA_KEY,
    STAR_SIGN_PROPERTY, UNKNOWN_STAR_SIGN,
};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Pipeline API", "https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-onprem.rs&utm_term=find-out-more"),
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-onprem.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-onprem.rs&utm_term=find-out-more"),
];

/// The evidence key the engine reads the birth date from.
const DATE_OF_BIRTH_EVIDENCE: &str = "date-of-birth";

/// The file name the example reads its boundaries from.
const DATA_FILE_NAME: &str = "starsigns.csv";

/// The data-source tier this engine reports. The on-premise star-sign data is a
/// free, illustrative file, so it reports the `free` tier.
const DATA_SOURCE_TIER: &str = "free";

/// An on-premise star-sign engine whose lookup boundaries come from a data file
/// rather than being compiled in.
///
/// This is the difference an "engine" makes over the simple element: the logic
/// (the date-to-sign boundaries) lives in a file that ships and updates
/// separately from the binary. The engine reads the file once at construction
/// into a reloadable table, so [`OnPremiseStarSignEngine::refresh`] can swap in a
/// newer file without rebuilding the pipeline, exactly as the production
/// device-detection and IP-intelligence on-premise engines reload their `.hash`
/// and `.ipi` data files.
struct OnPremiseStarSignEngine {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    data_file: PathBuf,
    /// The current boundary table, behind an `RwLock` so a reload can replace it
    /// while requests read it. This mirrors the synchronised reloadable data a
    /// real on-premise engine keeps.
    boundaries: RwLock<Vec<StarSignBoundary>>,
}

impl OnPremiseStarSignEngine {
    /// The typed key giving strongly-typed access to this engine's data.
    const KEY: TypedKey<StarSignData> = TypedKey::new(STAR_SIGN_DATA_KEY);

    /// Build the engine from a data file, reading its boundaries immediately.
    fn new(data_file: PathBuf) -> Result<Self> {
        let boundaries = read_boundaries(&data_file)?;
        Ok(OnPremiseStarSignEngine {
            filter: EvidenceKeyFilterWhitelist::new([DATE_OF_BIRTH_EVIDENCE]),
            properties: vec![PropertyMetaData::new(
                STAR_SIGN_PROPERTY,
                STAR_SIGN_DATA_KEY,
                PropertyValueType::String,
            )
            .with_category("StarSign")],
            data_file,
            boundaries: RwLock::new(boundaries),
        })
    }

    /// The data-source tier this engine reports.
    fn data_source_tier(&self) -> &str {
        DATA_SOURCE_TIER
    }

    /// Re-read the data file, replacing the in-memory boundary table. A real
    /// engine calls this from its file-system watcher or auto-update timer.
    fn refresh(&self) -> Result<()> {
        let boundaries = read_boundaries(&self.data_file)?;
        let mut guard = self
            .boundaries
            .write()
            .expect("the boundary lock is not poisoned");
        *guard = boundaries;
        Ok(())
    }
}

impl FlowElement for OnPremiseStarSignEngine {
    fn process(&self, data: &mut FlowData) -> Result<(), fiftyone_pipeline_core::Error> {
        let date = data
            .evidence()
            .get(DATE_OF_BIRTH_EVIDENCE)
            .map(str::to_owned);

        // Compute the sign against the current (reloadable) boundary table. The
        // read lock is released before the element data is borrowed mutably.
        let sign = {
            let boundaries = self
                .boundaries
                .read()
                .expect("the boundary lock is not poisoned");
            date.as_deref()
                .and_then(parse_day_month)
                .and_then(|(month, day)| star_sign_for(&boundaries, month, day))
                .unwrap_or(UNKNOWN_STAR_SIGN)
                .to_owned()
        };

        let star_sign_data = data.get_or_add(Self::KEY, StarSignData::new)?;
        star_sign_data.set_star_sign(sign);
        Ok(())
    }

    fn data_key(&self) -> &str {
        STAR_SIGN_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Read the boundary table from a CSV data file.
///
/// Each line is `name,start_month,start_day,end_month,end_day`. Blank lines and a
/// leading `#` comment line are skipped. The names are leaked into `'static`
/// strings so they fit the [`StarSignBoundary`] shared type; this is fine for an
/// example that loads the file once (or a handful of times), and keeps the shared
/// lookup zero-copy on the hot path.
fn read_boundaries(path: &Path) -> Result<Vec<StarSignBoundary>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the star-sign data file at {}", path.display()))?;
    let mut boundaries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if fields.len() != 5 {
            anyhow::bail!("malformed star-sign row '{line}', expected five comma-separated fields");
        }
        let name: &'static str = Box::leak(fields[0].to_owned().into_boxed_str());
        let start_month: u32 = fields[1].parse().context("start month")?;
        let start_day: u32 = fields[2].parse().context("start day")?;
        let end_month: u32 = fields[3].parse().context("end month")?;
        let end_day: u32 = fields[4].parse().context("end day")?;
        boundaries.push(StarSignBoundary {
            name,
            start: (start_month, start_day),
            end: (end_month, end_day),
        });
    }
    if boundaries.is_empty() {
        anyhow::bail!(
            "the star-sign data file at {} contained no rows",
            path.display()
        );
    }
    Ok(boundaries)
}

/// Write the default boundary table out as a CSV data file, so the example has
/// something to load when no file is supplied. A real engine never generates its
/// own data; this only exists so the example is self-contained and runnable with
/// no setup.
fn write_default_data_file(path: &Path) -> Result<()> {
    let mut text = String::from("# name,start_month,start_day,end_month,end_day\n");
    for sign in STAR_SIGNS {
        text.push_str(&format!(
            "{},{},{},{},{}\n",
            sign.name, sign.start.0, sign.start.1, sign.end.0, sign.end.1
        ));
    }
    std::fs::write(path, text).with_context(|| {
        format!(
            "writing the default star-sign data file at {}",
            path.display()
        )
    })?;
    Ok(())
}

/// Resolve the data file path: an explicit path if supplied, else a copy of the
/// default table written into the system temporary directory.
fn resolve_data_file(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let path = std::env::temp_dir().join(DATA_FILE_NAME);
    if !path.exists() {
        write_default_data_file(&path)?;
    }
    Ok(path)
}

/// Options controlling one run of the example.
pub struct ExampleOptions {
    /// The birth date as a `dd/mm/yyyy` string.
    pub date_of_birth: String,
    /// An explicit data file path. When `None`, a default file is written to a
    /// temporary location.
    pub data_file: Option<PathBuf>,
}

impl Default for ExampleOptions {
    fn default() -> Self {
        ExampleOptions {
            date_of_birth: "18/12/1992".to_owned(),
            data_file: None,
        }
    }
}

/// Run the example: build an on-premise engine over a data file, process a birth
/// date, then demonstrate reloading the data file.
///
/// This is a console example, so it does not add usage sharing; a production
/// application using 51Degrees on-premise data should enable it.
pub fn run(options: &ExampleOptions) -> Result<()> {
    let data_file = resolve_data_file(options.data_file.clone())?;
    println!("Reading star-sign boundaries from {}.", data_file.display());

    let engine = Arc::new(OnPremiseStarSignEngine::new(data_file)?);
    println!("Engine data-source tier: {}.", engine.data_source_tier());

    let pipeline: Arc<Pipeline> = Pipeline::builder()
        .add_element(engine.clone() as Arc<dyn FlowElement>)
        .build()?;

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(DATE_OF_BIRTH_EVIDENCE, options.date_of_birth.clone())
            .build(),
    );
    data.process()?;

    let sign = data
        .get(OnPremiseStarSignEngine::KEY)
        .and_then(StarSignData::star_sign)
        .unwrap_or(UNKNOWN_STAR_SIGN)
        .to_owned();
    println!(
        "With a date of birth of {}, your star sign is {sign}.",
        options.date_of_birth
    );

    // Demonstrate that the engine can reload its data file without the pipeline
    // being rebuilt, which is how a production engine picks up a data update.
    engine.refresh()?;
    println!("Reloaded the data file; the engine is ready to serve updated data.");
    Ok(())
}

/// Read an optional birth date and data file path from the command line, then run
/// the example.
fn main() -> Result<()> {
    let mut options = ExampleOptions::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(date) = args.first() {
        options.date_of_birth = date.clone();
    }
    if let Some(path) = args.get(1) {
        options.data_file = Some(PathBuf::from(path));
    }
    let result = run(&options);
    examples_shared::print_find_out_more(FIND_OUT_MORE, pipeline_examples::REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_with_default_options() {
        run(&ExampleOptions::default()).expect("the on-premise example should run");
    }

    #[test]
    fn reads_boundaries_from_a_written_file() {
        // Write a data file to a unique temporary path, then load it and check a
        // known date resolves correctly through the engine.
        let path = std::env::temp_dir().join("starsigns-onprem-test.csv");
        write_default_data_file(&path).unwrap();

        let engine = Arc::new(OnPremiseStarSignEngine::new(path.clone()).unwrap());
        let pipeline = Pipeline::builder()
            .add_element(engine as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(DATE_OF_BIRTH_EVIDENCE, "18/12/1992")
                .build(),
        );
        data.process().unwrap();
        assert_eq!(
            data.get(OnPremiseStarSignEngine::KEY)
                .and_then(StarSignData::star_sign),
            Some("Sagittarius")
        );

        let _ = std::fs::remove_file(path);
    }
}

/* ---------------------------------------------------------------------------
 * Example: On-Premise Engine (star sign from a data file)
 *
 * This example extends the simple flow-element example by moving the
 * date-to-sign boundaries out of the binary and into a data file the engine
 * reads. It is the smallest illustration of what an "on-premise engine" is: a
 * flow element whose logic is driven by a data file that ships and updates
 * independently of the application.
 *
 * What changes from the simple example
 * ------------------------------------
 * The element interface is unchanged: the same evidence key, the same property,
 * the same data key. The only difference is where the boundaries come from. The
 * engine:
 *
 *   1. Reads the boundary table from a CSV data file at construction
 *      (`read_boundaries`). The file format is one sign per line,
 *      `name,start_month,start_day,end_month,end_day`.
 *
 *   2. Keeps the table behind an `RwLock` so it can be replaced at runtime. The
 *      `refresh` method re-reads the file and swaps the table in. A production
 *      engine calls the equivalent from a file-system watcher or an auto-update
 *      timer, which is how the real 51Degrees device-detection and
 *      IP-intelligence engines pick up a new `.hash` or `.ipi` file without a
 *      restart.
 *
 *   3. Reports a data-source tier (`free` here), matching the way the production
 *      engines distinguish the Lite, ASN and Enterprise data tiers.
 *
 * Self-contained data
 * -------------------
 * So the example runs with no setup, it writes a default copy of the boundary
 * table to a temporary file when no data file is supplied. A real engine never
 * generates its own data; this only keeps the example runnable out of the box.
 * Supply a path as the second argument to point the engine at your own file.
 *
 * Usage sharing
 * -------------
 * This is a console example and therefore does NOT enable usage sharing. A
 * production on-premise deployment should; see the `usage-sharing` example.
 *
 * Running it
 * ----------
 *   cargo run -p pipeline-examples --bin ss-onprem [dd/mm/yyyy] [path/to/starsigns.csv]
 *
 * With no arguments it uses 18/12/1992 and a generated data file, and prints
 * "Sagittarius".
 * ------------------------------------------------------------------------- */
