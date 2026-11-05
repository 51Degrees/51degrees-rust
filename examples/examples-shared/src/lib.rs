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

//! # Shared example and test helpers
//!
//! The helper library used by every runnable example and example test in the
//! workspace. It gathers the shared example utilities (data-file location,
//! resource-key resolution, sample evidence and property formatting) into one
//! small crate so the examples themselves stay focused on the feature they show.
//!
//! It deliberately keeps a minimal dependency surface, only
//! [`fiftyone_pipeline_core`] and [`fiftyone_pipeline_engines`], so it can be
//! shared by Device Detection, IP Intelligence and pipeline examples alike
//! without pulling in either detection facade.
//!
//! ## What it provides
//!
//! - [`find_file`] / [`find_file_from`]. Walk up the directory tree to locate a
//!   data or resource file, so it resolves whether an example runs from an IDE,
//!   its own crate directory, or a CI checkout.
//! - [`resource_key_from_env`] and [`is_invalid_key`]. Read a cloud resource key
//!   from the aligned, legacy and CI-exported environment variables, and screen
//!   out obvious placeholders.
//! - [`cloud_endpoint_from_env`]. Read the optional cloud endpoint from
//!   `51DEGREES_CLOUD_ENDPOINT`, trimmed and screened for blankness, for an on
//!   premise web server or a privately hosted 51Degrees cloud (see
//!   [`CLOUD_ENDPOINT_ENV_VAR`]).
//! - [`dd_data_path`] and [`ipi_data_path`]. Resolve the on-premise Device
//!   Detection and IP Intelligence data files, the latter through the
//!   ([`IpiTier`]) Enterprise, ASN and Lite scheme.
//! - [`find_data_file`] / [`find_data_file_from`]. Find a free data file in
//!   the data repository, running the data repository's script to download it
//!   when it is missing.
//! - [`check_data_file`] and [`data_file_info`]. Inspect an on-premise engine's
//!   data file and return age and Lite-tier warnings for the example to print.
//! - [`get_property_as_string`]. Render any property from an element data bag to
//!   a display string, handling the missing and no-value cases.
//! - [`missing_properties`] and [`enterprise_data_file_message`]. Name the
//!   properties an example asks for that the data file in use does not carry,
//!   and give the example's link for asking about the Enterprise data file.
//! - [`print_find_out_more`] and [`write_find_out_more`]. The "Find out more"
//!   section that ends every example's output.
//! - [`PerformanceResults`] and [`json_output_path`]. The results model the
//!   performance examples emit for the nightly performance graphs, and the
//!   `--json-output` argument they take the path from.
//! - A set of sample evidence values (see the [`evidence`] module) covering the
//!   common detection paths.

#![warn(missing_docs)]

pub mod evidence;

mod data_file_check;
mod data_paths;
mod endpoint;
mod enterprise;
mod find_file;
mod find_out_more;
mod keys;
mod performance_results;
mod properties;

pub use data_file_check::{check_data_file, data_file_info, DATA_FILE_AGE_WARNING_DAYS};
pub use data_paths::{
    dd_data_path, find_data_file, find_data_file_from, ipi_data_path, latest_enterprise_ipi,
    IpiTier, DATA_DOWNLOAD_SOURCE, DD_LITE_RELATIVE_PATH, DD_PATH_ENV_VAR, IPI_ASN_RELATIVE_PATH,
    IPI_ENTERPRISE_FILE_NAME, IPI_ENTERPRISE_SHARE_ENV_VAR, IPI_LITE_RELATIVE_PATH,
    IPI_PATH_ENV_VAR,
};
pub use endpoint::{cloud_endpoint_from_env, CLOUD_ENDPOINT_ENV_VAR};
pub use enterprise::{
    enterprise_data_file_message, is_available, missing_properties, NEEDS_ENTERPRISE_DATA_FILE,
};
pub use find_file::{
    find_file, find_file_from, MAX_DESCENT_DEPTH, MAX_DIRECTORIES_SCANNED, MAX_PARENT_LEVELS,
};
pub use find_out_more::{
    print_find_out_more, write_find_out_more, CLOUD_REPOSITORIES, FIND_OUT_MORE_HEADING,
    ON_PREMISE_REPOSITORIES,
};
pub use performance_results::{json_output_path, PerformanceResults, JSON_OUTPUT_FLAG};
pub use keys::{
    is_invalid_key, resource_key_from_env, CI_RESOURCE_KEY_BASIC_ENV_VAR,
    CI_RESOURCE_KEY_BESPOKE_ENV_VAR, RESOURCE_KEY_ENV_VAR, RESOURCE_KEY_ENV_VARS,
};
pub use properties::{get_property_as_string, property_value_to_string, NO_VALUE_MARKER};
