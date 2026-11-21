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

//! Element data that gives a property's value as the string it was stored
//! as, beside the value parsed into the type the property declares.

use crate::AspectPropertyValue;

/// Element data that holds its property values as strings and parses each
/// into the type the property declares, and gives the stored string too.
///
/// A typed accessor answers in its own type, so a stored string that is not
/// one of that type's values still becomes one. A Boolean property storing
/// `Unknown` reads as `false`, which a caller cannot tell from a stored
/// `False`. The stored string is offered beside the typed accessors, which
/// are unchanged. Element data offers this through a method that gives
/// `Option<&dyn ProvidesValuesAsString>`, so a caller tests for it at run
/// time, and data that does not hold strings gives `None`. See
/// [values stored as strings](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/value-types.md#values-stored-as-strings).
pub trait ProvidesValuesAsString {
    /// The value stored for the named property, as it was stored. A
    /// property that is absent, has no value or that the request is not
    /// entitled to has no value, with the message the typed accessors give.
    fn value_as_string(&self, name: &str) -> AspectPropertyValue<String>;
}
