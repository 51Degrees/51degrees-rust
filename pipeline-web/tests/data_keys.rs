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

//! The string keys of the elements a web pipeline uses, checked against the
//! general guidance of the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/706de1f0cb3edd560eb7fd7ef3bb19b93a4883dc/pipeline-specification/features/access-to-results.md#L5)
//! for string keys.

use fiftyone_cloud_request_engine::ELEMENT_DATA_KEY as CLOUD_REQUEST_ELEMENT_DATA_KEY;
use fiftyone_javascript_builder::JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY;
use fiftyone_json_builder::JSON_BUILDER_ELEMENT_DATA_KEY;
use fiftyone_pipeline_engines_fiftyone::constants::{
    SEQUENCE_DEFAULT_ELEMENT_DATA_KEY, SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY,
    SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY,
};

/// The default element data key of each element.
const DATA_KEYS: [&str; 6] = [
    CLOUD_REQUEST_ELEMENT_DATA_KEY,
    JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
    JSON_BUILDER_ELEMENT_DATA_KEY,
    SEQUENCE_DEFAULT_ELEMENT_DATA_KEY,
    SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY,
    SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY,
];

/// "Strings SHOULD only contain alphanumeric characters, full stop or
/// hyphen." (access-to-results.md line 7)
#[test]
fn data_keys_only_contain_alphanumeric_characters_full_stop_or_hyphen() {
    for key in DATA_KEYS {
        assert!(!key.is_empty(), "a data key is empty");
        assert!(
            key.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'),
            "the data key '{key}' holds a character other than an \
             alphanumeric character, full stop or hyphen"
        );
    }
}
