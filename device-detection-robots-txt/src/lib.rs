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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-robots-txt-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-robots-txt-lib.rs&utm_term=logo)
//!
//! # robots.txt
//!
//! [`RobotsTxtEngine`] writes a robots.txt that allows or refuses each known
//! crawler by what it uses the pages it fetches for, such as search,
//! monitoring or training a model, from the crawlers a 51Degrees Device
//! Detection data file describes. It gives the text .NET's
//! `FiftyOne.DeviceDetection.RobotsTxt` gives, byte for byte.
//!
//! The text follows [RFC 9309](https://www.rfc-editor.org/rfc/rfc9309.html).
//! Each refused crawler has its own group with `Disallow: /`, and every
//! other crawler falls through to a closing `User-Agent: *` group with
//! `Allow: /`, which also names any Terms Document Locator (TDL) the request
//! gives.
//!
//! ## Use
//!
//! Add the engine after an on-premise Device Detection engine whose data
//! file holds the crawler properties `CrawlerName`, `CrawlerProductTokens`,
//! `CrawlerUrl` and `CrawlerUsage`, as the Enterprise file does. Then give
//! the evidence `query.robotstxt.<usage>` with `allow` or `disallow` for each
//! usage, and read `PlainText` or `AnnotatedText` from
//! [`RobotsTxtData`] under [`ROBOTS_TXT_DATA_KEY`]. A data file without the
//! crawler properties gives a text that allows every crawler.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
//! use fiftyone_device_detection_robots_txt::{RobotsTxtEngine, ROBOTS_TXT_DATA_KEY};
//! use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
//!
//! let device = DeviceDetectionOnPremiseEngineBuilder::new("Enterprise-HashV41.hash").build()?;
//! let robots = RobotsTxtEngine::builder().build()?;
//! let pipeline = Pipeline::builder()
//!     .add_element(device as Arc<dyn FlowElement>)
//!     .add_element(robots as Arc<dyn FlowElement>)
//!     .build()?;
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder()
//!         .add("query.robotstxt.search", "allow")
//!         .add("query.robotstxt.tdl", "MOW-SOCW")
//!         .build(),
//! );
//! data.process()?;
//! if let Some(text) = data
//!     .get(ROBOTS_TXT_DATA_KEY)
//!     .and_then(|robots| robots.plain_text().into_option())
//! {
//!     println!("{text}");
//! }
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```
//!
//! ## TDL source ids
//!
//! In place of a TDL URI the request can name a source, such as `MOW-SOCW`,
//! which a [`TdlSourceResolver`] turns into the latest URI. With the
//! `reqwest-client` feature the builder's default is a
//! [`GitHubTdlSourceResolver`] over the sources compiled in, which lists the
//! source's GitHub folder and keeps the answer for twelve hours. Without the
//! feature the engine accepts only URIs until
//! [`RobotsTxtEngineBuilder::set_tdl_source_resolver`] gives it a resolver.

#![warn(missing_docs)]

pub mod constants;
mod data;
mod engine;
mod github;
mod model;
mod notice;
mod tdl;

pub use data::{RobotsTxtData, ROBOTS_TXT_DATA_KEY};
pub use engine::{RobotsTxtEngine, RobotsTxtEngineBuilder};
#[cfg(feature = "reqwest-client")]
pub use github::ReqwestTdlHttpClient;
pub use github::{
    GitHubTdlSourceResolver, TdlHttpClient, TdlHttpResponse, DEFAULT_CACHE_TTL_SECONDS,
    GITHUB_API_URL, REQUEST_TIMEOUT_SECONDS,
};
pub use model::{CrawlerModel, RobotsTxtModel, UsageModel};
pub use notice::LICENSE_URL;
pub use tdl::{TdlSource, TdlSourceResolver, TdlSources, NAME_PLACEHOLDER, VERSION_GROUP_NAME};
