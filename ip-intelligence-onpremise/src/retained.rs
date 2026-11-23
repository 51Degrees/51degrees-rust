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

//! Values a refresh replaces that the engine lends by reference.

use std::sync::OnceLock;

/// Every distinct value kept so far, oldest first, of which the newest is the
/// current one.
///
/// The pipeline traits lend an engine's property lists and tier by reference
/// for as long as the engine lives, so a value a refresh replaces cannot be
/// freed while the engine lives. Each is kept instead. A refresh adds a value
/// only when it differs from the newest, which happens when a data file with
/// a different property set or tier is loaded.
pub(crate) struct Retained<T> {
    value: T,
    next: OnceLock<Box<Retained<T>>>,
}

impl<T: PartialEq> Retained<T> {
    pub(crate) fn new(value: T) -> Self {
        Retained {
            value,
            next: OnceLock::new(),
        }
    }

    fn newest_link(&self) -> &Retained<T> {
        let mut link = self;
        while let Some(next) = link.next.get() {
            link = next;
        }
        link
    }

    /// The newest value.
    pub(crate) fn current(&self) -> &T {
        &self.newest_link().value
    }

    /// Make `value` the current one, unless it equals the current one. The
    /// caller serialises calls, so two never add at once.
    pub(crate) fn replace(&self, value: T) {
        let newest = self.newest_link();
        if newest.value != value {
            let _ = newest.next.set(Box::new(Retained::new(value)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_distinct_value_is_current_and_older_ones_stay_lent() {
        let retained = Retained::new(String::from("Lite"));
        let lent = retained.current();
        retained.replace(String::from("Lite"));
        assert!(retained.next.get().is_none(), "an equal value is not kept");
        retained.replace(String::from("Enterprise"));
        assert_eq!(retained.current(), "Enterprise");
        assert_eq!(lent, "Lite");
        retained.replace(String::from("Lite"));
        assert_eq!(retained.current(), "Lite");
    }
}
