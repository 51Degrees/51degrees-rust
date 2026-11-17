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

//! A flow data, and the element data it holds, are not safe to use from
//! several threads at once, as the thread-safety specification's Flow Data
//! section says, so they are `Send` but not `Sync`. The compiler then refuses
//! code that shares one across threads, and a caller that must share one
//! wraps it in a lock of its own.

use std::sync::Mutex;

use fiftyone_pipeline_core::{ElementData, FlowData};

/// Implemented for every type, and a second time for every `Sync` type, so
/// naming the item through it is ambiguous, and fails to compile, exactly
/// when the type is `Sync`.
trait AmbiguousIfSync<A> {
    fn check() {}
}
impl<T: ?Sized> AmbiguousIfSync<()> for T {}
struct IsSync;
impl<T: ?Sized + Sync> AmbiguousIfSync<IsSync> for T {}

fn assert_send<T: Send + ?Sized>() {}
fn assert_sync<T: Sync + ?Sized>() {}

#[test]
fn a_flow_data_is_send_but_not_sync() {
    assert_send::<FlowData>();
    <FlowData as AmbiguousIfSync<_>>::check();
}

#[test]
fn element_data_is_send_but_not_sync() {
    assert_send::<dyn ElementData>();
    <dyn ElementData as AmbiguousIfSync<_>>::check();
}

#[test]
fn a_caller_shares_a_flow_data_through_a_lock_of_its_own() {
    assert_sync::<Mutex<FlowData>>();
}
