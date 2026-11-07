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

//! The flow element trait: a black box that processes flow data.
//!
//! A flow element reads evidence and earlier element data from a
//! [`crate::FlowData`] and may add its own element data. See the
//! [conceptual overview](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#flow-element).
//!
//! # Design choice: object-safe `dyn` trait, not an associated type
//!
//! A pipeline holds a heterogeneous, ordered list of elements that produce
//! different element-data types. To store them in a single `Vec` and run them
//! in order, [`FlowElement`] is kept **object-safe** so the pipeline can hold
//! `Arc<dyn FlowElement>` trait objects.
//!
//! That rules out putting the element-data type as an associated type or a
//! generic parameter on this trait (either would make `dyn FlowElement`
//! impossible). Instead the type linkage lives in [`crate::TypedKey<T>`]: a
//! concrete element exposes a `TypedKey<T>` for its own data through its own
//! inherent API, and [`crate::FlowData::get`] uses it to downcast. This is the
//! "no-reflection core" approach from the plan and realises mechanisms 1 and 2
//! of the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md)
//! without a generic on the stored element.
//!
//! # Thread safety
//!
//! `process` takes `&self`, so one shared element instance serves many
//! concurrent flow data, as required by the
//! [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#flow-elements).
//! The trait is therefore `Send + Sync`. Any per-request scratch state must be
//! created inside `process`; any hot mutable state (for example a reloadable
//! data file) belongs behind a synchronisation primitive owned by the element.

use std::any::Any;
use std::sync::Arc;

use crate::error::Result;
use crate::evidence::EvidenceKeyFilter;
use crate::flow_data::FlowData;
use crate::pipeline::Pipeline;
use crate::property::PropertyMetaData;

/// The basic building block of a pipeline.
///
/// Implementations are shared (`Arc`) and immutable once added to a pipeline,
/// so the trait requires `Send + Sync`. See the module documentation for the
/// rationale behind the object-safe design.
pub trait FlowElement: Send + Sync {
    /// Process the supplied flow data with this element.
    ///
    /// The element reads evidence and earlier element data from `data` and may
    /// add its own element data via [`crate::FlowData::get_or_add`]. Returning
    /// `Err` signals a processing failure; the pipeline decides whether to
    /// propagate or record it according to its `suppress_process_exceptions`
    /// setting, per the
    /// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#process-function).
    ///
    /// `process` takes `&self` so a single element instance can serve many
    /// concurrent requests.
    fn process(&self, data: &mut FlowData) -> Result<()>;

    /// The string data key used to store and retrieve this element's data in a
    /// flow data. This is a specification MUST.
    fn data_key(&self) -> &str;

    /// A filter describing the evidence keys this element can make use of.
    ///
    /// Advertising accepted evidence is a specification MUST. See the
    /// [advertise-accepted-evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/advertize-accepted-evidence.md).
    /// The pipeline ORs every element's filter together to derive its own
    /// pipeline-wide filter.
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter;

    /// True once [`FlowElement::evidence_key_filter`] holds every key this
    /// element will accept. Defaults to `true`.
    ///
    /// An element that learns its accepted evidence after it is built, as the
    /// cloud request engine does after a transient discovery failure, returns
    /// `false` until then. A consumer that keeps the filter's keys, such as the
    /// web `Vary` list, collects them again until this is `true`, because the
    /// [cloud request engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#consequences-for-consumers-of-the-engine)
    /// forbids keeping a result obtained before discovery completed. An
    /// element that wraps another forwards this.
    fn is_evidence_key_filter_complete(&self) -> bool {
        true
    }

    /// Metadata for the properties this element can populate.
    ///
    /// Publishing produced properties is a specification MUST. See the
    /// [properties specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#property-metadata).
    fn properties(&self) -> &[PropertyMetaData];

    /// True once [`FlowElement::properties`] holds this element's own
    /// property list. Defaults to `true`.
    ///
    /// An element that learns its properties after it is built, as a cloud
    /// aspect engine does from the cloud request engine's discovery, returns
    /// `false` until then, and the list it gives meanwhile may be defaults. A
    /// consumer deciding what the pipeline offers leaves such an element out
    /// and asks again later. An element that wraps another forwards this.
    ///
    /// An aspect engine that can answer `false` from its
    /// `has_loaded_properties` overrides this as well, reading its property
    /// list first and then answering `has_loaded_properties`, because this
    /// default would otherwise count it as loaded before its list is.
    fn properties_loaded(&self) -> bool {
        true
    }

    /// True if this element starts multiple threads internally. Defaults to
    /// `false`. A pipeline reports itself concurrent if any element does, which
    /// influences whether thread-safe flow data is needed.
    fn is_concurrent(&self) -> bool {
        false
    }

    /// The fully qualified name of the element's type, for example
    /// `fiftyone_pipeline_engines_fiftyone::share_usage_element::ShareUsageElement`.
    ///
    /// Usage sharing reports it for each element in the pipeline. The default
    /// is the implementing type's name, so an element only overrides this when
    /// it wraps another and should report the inner type.
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Called by [`crate::PipelineBuilder::build`] once for each place this
    /// element has in the pipeline being built, after the pipeline is made and
    /// before it is returned. An error fails the build. The default does
    /// nothing.
    ///
    /// One element instance can be added to several pipelines. An element that
    /// keeps state for its pipeline must keep that state for each pipeline, or
    /// refuse a second pipeline here, as the
    /// [multi-pipeline elements specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/advanced-features/multi-pipeline-elements.md)
    /// says. Hold the pipeline as a [`std::sync::Weak`], because the pipeline
    /// holds the element and a clone would keep both alive. An element that
    /// wraps another passes the call on to it.
    fn add_pipeline(&self, _pipeline: &Arc<Pipeline>) -> Result<()> {
        Ok(())
    }

    /// This element as [`Any`], so code holding it as `Arc<dyn FlowElement>`
    /// can get back its own type. Defaults to `None`.
    ///
    /// An element whose own type a consumer needs returns `Some(self)`. The
    /// JSON and JavaScript builders do, so the web integration can hold a
    /// builder the application gives and build a response again with it. An
    /// element that wraps another does not pass this on, because the wrapper
    /// is the element in the pipeline, not the one it wraps.
    fn into_any(self: Arc<Self>) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::EvidenceKeyFilterWhitelist;

    /// An element that overrides none of the provided methods.
    struct Plain {
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for Plain {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            Ok(())
        }

        fn data_key(&self) -> &str {
            "plain"
        }

        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }

        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    #[test]
    fn an_element_has_its_properties_loaded_unless_it_says_otherwise() {
        let element = Plain {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        };
        assert!(element.properties_loaded());
        let element: &dyn FlowElement = &element;
        assert!(element.properties_loaded());
    }

    #[test]
    fn an_element_gives_back_no_type_unless_it_says_otherwise() {
        let element: Arc<dyn FlowElement> = Arc::new(Plain {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        });
        assert!(element.into_any().is_none());
    }
}
