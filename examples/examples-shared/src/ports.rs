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

//! The port a web example listens on, read from the environment.

/// The environment variable giving the port, or ports, a web example listens
/// on. Each web example here listens on one port, so it takes the first of
/// a list.
pub const EXAMPLE_HTTP_PORTS_ENV_VAR: &str = "_51DEGREES_EXAMPLE_HTTP_PORTS";

/// The port a web example listens on: the first port
/// [`EXAMPLE_HTTP_PORTS_ENV_VAR`] gives, or `default` when the variable is
/// unset or its first entry is not a port.
pub fn example_http_port(default: u16) -> u16 {
    first_port(std::env::var(EXAMPLE_HTTP_PORTS_ENV_VAR).ok().as_deref()).unwrap_or(default)
}

/// The first entry of a list of ports separated by `;` or `,`, when it is a
/// port.
fn first_port(value: Option<&str>) -> Option<u16> {
    value?.split([';', ',']).next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_port_is_read() {
        assert_eq!(first_port(Some("8095")), Some(8095));
        assert_eq!(first_port(Some(" 8095 ")), Some(8095));
    }

    #[test]
    fn the_first_of_a_list_is_taken() {
        assert_eq!(first_port(Some("5100;5101")), Some(5100));
        assert_eq!(first_port(Some("5100, 5101")), Some(5100));
    }

    #[test]
    fn nothing_that_is_not_a_port_is_read() {
        assert_eq!(first_port(None), None);
        assert_eq!(first_port(Some("")), None);
        assert_eq!(first_port(Some("http")), None);
        assert_eq!(first_port(Some("70000")), None);
        assert_eq!(first_port(Some(";8095")), None);
    }

    #[test]
    fn the_name_starts_as_every_51degrees_name_does() {
        assert!(EXAMPLE_HTTP_PORTS_ENV_VAR.starts_with("_51DEGREES_"));
    }
}
