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

//! # 51Degrees example benchmarks
//!
//! Criterion micro-benchmarks that guard the throughput and render budgets the
//! Device Detection and IP Intelligence specifications call for. They are
//! expressed as repeatable Criterion benches so a regression shows up as a
//! measured slowdown rather than as eyeballing console output.
//!
//! The crate holds no library code of its own. Its targets are the three benches
//! under `benches/`, each a self-contained file wired with `harness = false`:
//!
//! - `dd_onprem_throughput`. On-premise Device Detection over
//!   "20000 Evidence Records.yml" against the Lite `.hash` file, reporting
//!   detections per second.
//! - `ipi_onprem_throughput`. On-premise IP Intelligence lookups over
//!   `evidence.yml` against the 4.5 ASN `.ipi` file, reporting lookups per second.
//! - `javascript_builder_render`. The JavaScript-builder template render time,
//!   with the minify path measured too when the `minify` feature is enabled.
//!
//! Every bench resolves its inputs through [`examples_shared`] and skips itself
//! cleanly (registering no Criterion benchmark) when an input is missing, so a
//! plain `cargo bench` is safe to run in a checkout without the data submodules
//! or the production data share.
