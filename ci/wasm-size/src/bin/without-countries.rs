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

//! A pipeline with `CountryCodeTranslationEngine` alone, the smaller of the
//! two programs `ci/measure-wasm-size.sh` compares.
//!
//! The language comes from the first argument, so the build cannot know it.

use std::error::Error;
use std::sync::Arc;

use fiftyone_ip_intelligence_translation::CountryCodeTranslationEngine;
use fiftyone_pipeline_core::{Evidence, Pipeline};

fn main() -> Result<(), Box<dyn Error>> {
    let language = std::env::args().nth(1).unwrap_or_default();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(CountryCodeTranslationEngine::builder().build()?))
        .build()?;
    let evidence = Evidence::builder()
        .add("query.translation", language)
        .build();
    let mut data = pipeline.create_flow_data_with(evidence);
    // With no IP Intelligence data the engine records that its source is
    // missing, so the result is printed rather than returned.
    println!("{:?}", data.process());
    Ok(())
}
