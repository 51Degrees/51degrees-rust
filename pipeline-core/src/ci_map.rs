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

//! Shared case-insensitive lookup helper for the crate's lowercased-key maps.
//!
//! Several stores in this crate ([`crate::Evidence`], [`crate::FlowData`] and
//! [`crate::MapElementData`]) keep their keys lowercased on insertion and then
//! match lookups case-insensitively. [`ci_lookup`] is the single
//! implementation of that "try the key as supplied, then its lowercased form"
//! rule, so every site behaves identically, and [`ci_get`] applies it to a
//! `HashMap`.

use std::collections::HashMap;

use ahash::RandomState;

/// Look up `key` through `get`, which finds an exact key in a store whose
/// keys are stored lowercased, matching case-insensitively.
///
/// The fast path tries the key exactly as supplied (most keys are already
/// lowercase, so this hits without allocating). Only on a miss for a key that
/// has uppercase letters does it allocate a lowercased copy and retry, because
/// a lowercase key that missed would miss again.
pub(crate) fn ci_lookup<'a, V>(key: &str, get: impl Fn(&str) -> Option<&'a V>) -> Option<&'a V> {
    get(key).or_else(|| {
        let has_uppercase = !key.is_ascii() || key.bytes().any(|byte| byte.is_ascii_uppercase());
        if has_uppercase {
            get(key.to_lowercase().as_str())
        } else {
            None
        }
    })
}

/// Look up `key` in a map whose keys are stored lowercased, matching
/// case-insensitively by [`ci_lookup`]'s rule.
pub(crate) fn ci_get<'a, V>(map: &'a HashMap<String, V, RandomState>, key: &str) -> Option<&'a V> {
    ci_lookup(key, |exact| map.get(exact))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowercased_map() -> HashMap<String, u32, RandomState> {
        let mut map = HashMap::with_hasher(RandomState::new());
        map.insert("ismobile".to_owned(), 1);
        map.insert("ärger".to_owned(), 2);
        map
    }

    #[test]
    fn finds_a_key_in_any_case() {
        let map = lowercased_map();
        assert_eq!(ci_get(&map, "ismobile"), Some(&1));
        assert_eq!(ci_get(&map, "IsMobile"), Some(&1));
        assert_eq!(ci_get(&map, "ÄRGER"), Some(&2));
    }

    #[test]
    fn a_missing_key_is_none_in_any_case() {
        let map = lowercased_map();
        assert_eq!(ci_get(&map, "platformname"), None);
        assert_eq!(ci_get(&map, "PlatformName"), None);
    }
}
