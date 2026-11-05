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

//! Runnable pipeline examples.
//!
//! The example binaries live in `src/bin/` and are auto-discovered by Cargo, so
//! each is fully self-contained and they can be added independently without
//! touching this file. The handful of pieces every star-sign example needs (the
//! star-sign element data type, the boundary table and the date parsing) are
//! gathered here in [`star_sign`] so the bins share one tested implementation
//! rather than copying it six times.
//!
//! The examples implement the core flow-element demonstrations the
//! [pipeline specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/required-examples.md)
//! requires: determining a star sign from a birth date with hard-coded logic, an
//! on-premise data file, client-side JavaScript evidence and a cloud service,
//! plus the usage-sharing and caching feature examples.

#![warn(missing_docs)]

pub mod star_sign;

/// The repositories the pipeline examples list in their "Find out more"
/// section, being this SDK and the specifications its pipeline follows.
pub const REPOSITORIES: &[&str] = &[
    "https://github.com/51Degrees/51degrees-rust",
    "https://github.com/51Degrees/specifications",
];
