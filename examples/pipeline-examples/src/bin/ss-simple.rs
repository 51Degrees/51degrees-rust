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

//! Simple flow element: determine a star sign from a birth date with hard-coded
//! logic.

use std::sync::Arc;

use anyhow::Result;
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValueType, TypedKey,
};
use pipeline_examples::star_sign::{
    star_sign_for, StarSignBoundary, StarSignData, STAR_SIGNS, STAR_SIGN_DATA_KEY,
    STAR_SIGN_PROPERTY, UNKNOWN_STAR_SIGN,
};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Pipeline API", "https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-simple.rs&utm_term=find-out-more"),
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-simple.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-simple.rs&utm_term=find-out-more"),
];

/// The evidence key the example reads the birth date from.
const DATE_OF_BIRTH_EVIDENCE: &str = "date-of-birth";

/// A flow element that determines a star sign from a `date-of-birth` evidence
/// value using a hard-coded table of sign boundaries.
///
/// This is the simplest possible custom element: it accepts one evidence key,
/// produces one property and carries its lookup table in the binary. The richer
/// examples replace that table with an external data file, a client-side
/// JavaScript prompt or a remote cloud service, but the element shape is the
/// same throughout.
struct SimpleStarSignElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    boundaries: Vec<StarSignBoundary>,
}

impl SimpleStarSignElement {
    /// The typed key giving strongly-typed access to this element's data.
    const KEY: TypedKey<StarSignData> = TypedKey::new(STAR_SIGN_DATA_KEY);

    /// Build the element, advertising the one evidence key it reads and the one
    /// property it populates, and capturing the hard-coded boundary table.
    fn new() -> Self {
        SimpleStarSignElement {
            filter: EvidenceKeyFilterWhitelist::new([DATE_OF_BIRTH_EVIDENCE]),
            properties: vec![PropertyMetaData::new(
                STAR_SIGN_PROPERTY,
                STAR_SIGN_DATA_KEY,
                PropertyValueType::String,
            )],
            boundaries: STAR_SIGNS.to_vec(),
        }
    }
}

impl FlowElement for SimpleStarSignElement {
    fn process(&self, data: &mut FlowData) -> Result<(), fiftyone_pipeline_core::Error> {
        // Read the birth date from evidence. The simple example accepts the date
        // as a `dd/mm/yyyy` string, the same shape the other examples use, so the
        // bins all agree on the evidence format.
        let date = data
            .evidence()
            .get(DATE_OF_BIRTH_EVIDENCE)
            .map(str::to_owned);

        // Determine the sign (falling back to the "Unknown" marker) before we
        // borrow the element data mutably, so the lookup borrows nothing from it.
        let sign = date
            .as_deref()
            .and_then(pipeline_examples::star_sign::parse_day_month)
            .and_then(|(month, day)| star_sign_for(&self.boundaries, month, day))
            .unwrap_or(UNKNOWN_STAR_SIGN)
            .to_owned();

        // Add (or fetch) this element's data and record the sign.
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

/// Options controlling one run of the example.
pub struct ExampleOptions {
    /// The birth date as a `dd/mm/yyyy` string.
    pub date_of_birth: String,
}

impl Default for ExampleOptions {
    fn default() -> Self {
        // 18 December 1992, which resolves to Sagittarius.
        ExampleOptions {
            date_of_birth: "18/12/1992".to_owned(),
        }
    }
}

/// Run the example: build a one-element pipeline, process a birth date and print
/// the resulting star sign.
///
/// Note that a production pipeline that talks to 51Degrees services would add the
/// usage-sharing element (see the `usage-sharing` example); a console example
/// deliberately does not, so running it sends no data anywhere.
pub fn run(options: &ExampleOptions) -> Result<()> {
    // Build the pipeline from the single hard-coded element.
    let pipeline: Arc<Pipeline> = Pipeline::builder()
        .add_element(Arc::new(SimpleStarSignElement::new()))
        .build()?;

    // Create a flow data, add the birth date as evidence and process it.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(DATE_OF_BIRTH_EVIDENCE, options.date_of_birth.clone())
            .build(),
    );
    data.process()?;

    // Read the result back through the typed key and print it.
    let sign = data
        .get(SimpleStarSignElement::KEY)
        .and_then(StarSignData::star_sign)
        .unwrap_or(UNKNOWN_STAR_SIGN)
        .to_owned();
    println!(
        "With a date of birth of {}, your star sign is {sign}.",
        options.date_of_birth
    );
    Ok(())
}

/// Read an optional birth date from the first command-line argument, then run the
/// example.
fn main() -> Result<()> {
    let mut options = ExampleOptions::default();
    if let Some(date) = std::env::args().nth(1) {
        options.date_of_birth = date;
    }
    let result = run(&options);
    let _ = (FIND_OUT_MORE, pipeline_examples::REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_with_default_options() {
        run(&ExampleOptions::default()).expect("the simple example should run");
    }

    #[test]
    fn computes_the_expected_sign() {
        // Drive the element directly to assert the computed value, not just that
        // the example completes.
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(SimpleStarSignElement::new()))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(DATE_OF_BIRTH_EVIDENCE, "18/12/1992")
                .build(),
        );
        data.process().unwrap();
        assert_eq!(
            data.get(SimpleStarSignElement::KEY)
                .and_then(StarSignData::star_sign),
            Some("Sagittarius")
        );
    }
}

/* ---------------------------------------------------------------------------
 * Example: Simple Flow Element (star sign from a birth date)
 *
 * This example is the starting point for the 51Degrees pipeline custom
 * flow-element examples. It shows the minimum needed to add your own logic to a
 * pipeline: define a flow element, declare the evidence it reads and the
 * properties it writes, build a pipeline around it and read the result back.
 *
 * What it demonstrates
 * --------------------
 * A flow element is the basic unit of work in a pipeline. This one,
 * `SimpleStarSignElement`, takes a single piece of evidence (a date of birth)
 * and produces a single property (the corresponding star sign). The mapping from
 * date to sign is hard-coded in the binary as a table of date boundaries.
 *
 * Every flow element does four things, all visible above:
 *
 *   1. Advertises the evidence it can use. `evidence_key_filter` returns a
 *      whitelist containing `date-of-birth`. The pipeline ORs every element's
 *      filter together, which is how a web integration knows which request
 *      values matter (and therefore what to vary its cache on).
 *
 *   2. Publishes the properties it populates. `properties` lists the `starsign`
 *      property and its value type. Publishing metadata lets tooling and the
 *      cloud and web layers discover what an element produces without running it.
 *
 *   3. Names its data. `data_key` returns `starsign`; this is the key the
 *      element's data is stored under in the flow data, and the string form of
 *      the strongly-typed `KEY`.
 *
 *   4. Processes the flow data. `process` reads the evidence, computes the sign
 *      and writes it to the element's own data via `get_or_add`, which is where
 *      the caller and later elements read an element's results.
 *
 * Reading the result
 * ------------------
 * After `process`, the example reads the sign with the strongly-typed `KEY`,
 * which downcasts the stored data to `StarSignData` with no reflection. The same
 * value is also available dynamically by name through `flow_data.get_str(...)`.
 *
 * Usage sharing
 * -------------
 * This is a console example, so it deliberately does NOT add the usage-sharing
 * element. A production application that uses 51Degrees on-premise or cloud data
 * should enable usage sharing so detection accuracy keeps improving; see the
 * `usage-sharing` example for how to add it.
 *
 * Running it
 * ----------
 *   cargo run -p pipeline-examples --bin ss-simple [dd/mm/yyyy]
 *
 * With no argument it uses 18/12/1992 and prints "Sagittarius".
 * ------------------------------------------------------------------------- */
