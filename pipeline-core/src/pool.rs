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

//! Element data kept for reuse by later requests, on any thread.
//!
//! [`crate::FlowData::get_or_add_pooled`] takes an instance from the pool for
//! its type, and a flow data gives each element data that
//! [`ElementData::reset`] clears back to the pool for its type when it is
//! dropped. Every thread shares one pool for each type, because a web server
//! often processes a request on one thread and drops its flow data on
//! another. A pool is a fixed number of slots, each holding one instance or
//! none, so taking and giving take no lock. An atomic swap empties a slot and
//! an atomic compare and swap fills an empty one. A pool keeps at most
//! [`MAX_KEPT_PER_TYPE`] instances. Only a type that some element takes with
//! `get_or_add_pooled` has a pool, and at most [`MAX_POOLED_TYPES`] do.

use std::any::{Any, TypeId};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

use crate::element_data::ElementData;

/// The most instances of one element data type a pool keeps.
pub(crate) const MAX_KEPT_PER_TYPE: usize = 64;

/// The most element data types that have a pool.
const MAX_POOLED_TYPES: usize = 64;

/// The pools, in the order their types were first taken. A pool is only
/// created in the first empty place, so the pools fill the places in order.
/// Creating a type's pool, once, is the only time a thread can wait.
static POOLS: [OnceLock<Pool>; MAX_POOLED_TYPES] = [const { OnceLock::new() }; MAX_POOLED_TYPES];

/// The pool of one element data type.
struct Pool {
    type_id: TypeId,
    kept: Box<dyn Keep>,
}

/// Keeps element data of one type.
trait Keep: Send + Sync {
    /// Keep `data`, which is of the pool's type, if a slot is empty, or
    /// drop it.
    fn keep(&self, data: Box<dyn ElementData>);

    /// This, for downcasting to the slots of the pool's type.
    fn as_any(&self) -> &dyn Any;
}

/// The slots of the pool of `T`, each holding a pointer from
/// [`Box::into_raw`] to an instance kept, or null.
struct Slots<T> {
    slots: [AtomicPtr<T>; MAX_KEPT_PER_TYPE],
}

impl<T: ElementData> Slots<T> {
    fn new() -> Self {
        Slots {
            slots: std::array::from_fn(|_| AtomicPtr::new(ptr::null_mut())),
        }
    }

    /// An instance kept, emptying its slot, or `None` when every slot is
    /// empty.
    fn take(&self) -> Option<Box<T>> {
        self.slots.iter().find_map(|slot| {
            if slot.load(Ordering::Relaxed).is_null() {
                return None;
            }
            let kept = slot.swap(ptr::null_mut(), Ordering::Acquire);
            if kept.is_null() {
                return None;
            }
            // Safety: a slot only holds null or a pointer `keep` gave up with
            // `Box::into_raw`, and the swap gives that pointer to this call
            // alone.
            Some(unsafe { Box::from_raw(kept) })
        })
    }
}

impl<T: ElementData> Keep for Slots<T> {
    fn keep(&self, data: Box<dyn ElementData>) {
        let data: Box<dyn Any> = data;
        let Ok(data) = data.downcast::<T>() else {
            return;
        };
        let raw = Box::into_raw(data);
        for slot in &self.slots {
            if slot
                .compare_exchange(ptr::null_mut(), raw, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
        // Safety: no slot took `raw`, so this call still owns it.
        drop(unsafe { Box::from_raw(raw) });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The pool of `T`, created when `T` has none and a place is free.
fn pool_of<T: ElementData>() -> Option<&'static Pool> {
    let type_id = TypeId::of::<T>();
    POOLS
        .iter()
        .map(|place| {
            place.get_or_init(|| Pool {
                type_id,
                kept: Box::new(Slots::<T>::new()),
            })
        })
        .find(|pool| pool.type_id == type_id)
}

/// The pool of the type `type_id`, or `None` when it has none.
fn find(type_id: TypeId) -> Option<&'static Pool> {
    POOLS
        .iter()
        .map_while(OnceLock::get)
        .find(|pool| pool.type_id == type_id)
}

/// An instance of `T` a flow data gave back, cleared by
/// [`ElementData::reset`], or `None` when the pool of `T` holds none.
pub(crate) fn take<T: ElementData>() -> Option<Box<dyn ElementData>> {
    let slots = pool_of::<T>()?.kept.as_any().downcast_ref::<Slots<T>>()?;
    slots.take().map(|data| data as Box<dyn ElementData>)
}

/// Keep `data` for reuse when its type has a pool with an empty slot and
/// [`ElementData::reset`] clears it. Otherwise it is dropped.
pub(crate) fn give(mut data: Box<dyn ElementData>) {
    let Some(pool) = find(data.as_any().type_id()) else {
        return;
    };
    if data.reset() {
        pool.kept.keep(data);
    }
}
