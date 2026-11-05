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

//! A caller's conversion of property values to JSON.

use fiftyone_pipeline_core::PropertyValue;

use crate::element::JsonBuilderElement;
use crate::value::JsonValue;

/// Converts a property value to JSON in place of the builder's own
/// conversion.
///
/// The builder asks its converters, in the order they were added, for each
/// property value it writes, including those in a list of records, and uses
/// the first answer. When every converter answers `None` the builder's own
/// conversion is used. Add converters with
/// [`JsonBuilderElementBuilder::add_json_converter`](crate::JsonBuilderElementBuilder::add_json_converter).
///
/// These match the extra `JsonConverter` instances given to .NET's
/// `JsonBuilderElementBuilder.SetJsonConverters`. A closure taking the
/// element and the value is also a converter.
pub trait JsonValueConverter: Send + Sync {
    /// The JSON for the value, or `None` to leave it to the next converter.
    ///
    /// Call [`JsonBuilderElement::to_json_value`] to convert a value held
    /// inside this one.
    fn convert(&self, element: &JsonBuilderElement, value: &PropertyValue) -> Option<JsonValue>;
}

impl<F> JsonValueConverter for F
where
    F: Fn(&JsonBuilderElement, &PropertyValue) -> Option<JsonValue> + Send + Sync,
{
    fn convert(&self, element: &JsonBuilderElement, value: &PropertyValue) -> Option<JsonValue> {
        self(element, value)
    }
}
