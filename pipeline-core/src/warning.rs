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

//! Warnings, being messages for the caller about a request that was served
//! even though something about it was wrong.

/// Something the caller should know about a request that did not stop it
/// being served, such as an evidence value an element could not use.
///
/// Record one with [`crate::FlowData::add_warning`] and read them with
/// [`crate::FlowData::warnings`]. Unlike a [`crate::FlowError`], a warning
/// never affects processing, so an element can explain a problem the caller
/// can act on without failing the request. The JSON builder writes the
/// messages under `warnings`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowWarning {
    /// The message for the caller.
    pub message: String,
    /// The data key of the element the warning relates to, or `None` when it
    /// did not come from an element.
    pub element_data_key: Option<String>,
}

impl FlowWarning {
    /// A warning with its message and, when it came from an element, that
    /// element's data key.
    pub fn new(message: impl Into<String>, element_data_key: Option<String>) -> Self {
        FlowWarning {
            message: message.into(),
            element_data_key,
        }
    }
}
