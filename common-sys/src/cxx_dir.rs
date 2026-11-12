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

//! Which environment variable points the build at a 51degrees-cxx checkout.
//! The build script includes this file by path, and so does the test in
//! `tests/cxx_dir.rs`, because a test cannot call a build script. It is not
//! a module of the library and uses the standard library alone.

/// The variable a developer sets to build from a checkout.
pub const CXX_DIR_VARIABLE: &str = "_51DEGREES_CXX_DIR";

/// The deprecated name [`CXX_DIR_VARIABLE`] replaced, read only when
/// [`CXX_DIR_VARIABLE`] is not set.
pub const OLD_CXX_DIR_VARIABLE: &str = "FIFTYONE_CXX_DIR";

/// The checkout the environment names and the variable that named it, or
/// `None` when neither variable is set. [`CXX_DIR_VARIABLE`] wins when both
/// are set. `read` gives the value of a variable.
pub fn cxx_dir_override(read: impl Fn(&str) -> Option<String>) -> Option<(String, &'static str)> {
    [CXX_DIR_VARIABLE, OLD_CXX_DIR_VARIABLE]
        .into_iter()
        .find_map(|name| read(name).map(|dir| (dir, name)))
}
