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

//! Element data kept for reuse by later requests on the same thread.
//!
//! A flow data gives each element data that [`ElementData::reset`] clears to
//! the pool of the thread that drops it, and
//! [`crate::FlowData::get_or_add_pooled`] takes one from the pool of the thread
//! that processes. Each thread keeps its own pool, so taking and giving take no
//! lock, and keeps at most [`MAX_KEPT_PER_TYPE`] instances of each type.

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::element_data::ElementData;

/// The most instances of one element data type a thread keeps.
pub(crate) const MAX_KEPT_PER_TYPE: usize = 64;

thread_local! {
    static KEPT: RefCell<HashMap<TypeId, Vec<Box<dyn ElementData>>>> =
        RefCell::new(HashMap::new());
}

/// An instance of `T` this thread kept, cleared by [`ElementData::reset`].
pub(crate) fn take<T: ElementData>() -> Option<Box<dyn ElementData>> {
    KEPT.try_with(|kept| kept.borrow_mut().get_mut(&TypeId::of::<T>())?.pop())
        .ok()
        .flatten()
}

/// Keep `data` for reuse on this thread if [`ElementData::reset`] clears it
/// and the thread keeps fewer than [`MAX_KEPT_PER_TYPE`] of its type.
/// Otherwise it is dropped, outside the pool, so its own drop can give data
/// back too.
pub(crate) fn give(mut data: Box<dyn ElementData>) {
    if !data.reset() {
        return;
    }
    let type_id = data.as_any().type_id();
    let refused = KEPT.try_with(|kept| {
        let mut kept = kept.borrow_mut();
        let of_type = kept.entry(type_id).or_default();
        if of_type.len() < MAX_KEPT_PER_TYPE {
            of_type.push(data);
            None
        } else {
            Some(data)
        }
    });
    drop(refused);
}
