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

//! What the engine gives for a value it has no translation for.

/// What the engine gives for a value it has no translation for, or for every
/// value when the request names no language the engine has.
///
/// A translation that is blank in its source file counts as no translation,
/// so a file can list the values still to be translated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MissingTranslationBehavior {
    /// Give the value as it was. The default.
    #[default]
    Original,
    /// Give an empty string.
    EmptyString,
    /// Give no value, and add a flow error saying why.
    FlowError,
}
