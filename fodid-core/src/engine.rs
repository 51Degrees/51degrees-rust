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

//! The trait an engine producing 51Dids implements.

use fiftyone_pipeline_engines::AspectEngine;

/// An engine that produces 51Dids, the counterpart of .NET's `IDidEngine`.
///
/// When it processes a flow data, an implementation stores a
/// [`FodIdDataBase`](crate::FodIdDataBase) under
/// [`FODID_DATA_KEY`](crate::FODID_DATA_KEY), and its
/// [`data_key`](fiftyone_pipeline_core::FlowElement::data_key) is
/// [`FODID_ELEMENT_DATA_KEY`](crate::FODID_ELEMENT_DATA_KEY). A consumer
/// therefore reads the result the same way whichever engine produced it.
///
/// The trait adds no methods to [`AspectEngine`], as `IDidEngine` adds none
/// to `IAspectEngine`. It names the engines that produce 51Dids, so code can
/// accept any of them as a type bound or as `dyn FodIdEngine`. An
/// `Arc<dyn FodIdEngine>` is added to a pipeline as it is, because it
/// converts to `Arc<dyn FlowElement>`.
///
/// The cloud engine in `fiftyone-fodid-cloud` implements it. An engine that
/// does not use the cloud implements it from this crate alone, with no cloud
/// request engine.
pub trait FodIdEngine: AspectEngine {}
