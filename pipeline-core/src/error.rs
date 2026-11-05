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

//! The pipeline error model.
//!
//! This follows the custom exception hierarchy described in the
//! [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md),
//! expressed as a single [`enum@Error`] enum so callers can match on the cause.
//!
//! Two distinct "value is absent" conditions are kept separate, because they
//! mean different things to the caller and carry different remedies:
//!
//! - [`NoValueError`] means the property exists in the result set but the
//!   element chose not to set a value (for example, device detection could not
//!   determine the value from the supplied evidence). This is the
//!   ["null values" rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values).
//! - [`Error::PropertyMissing`] means the property is not present in the result
//!   set at all (for example, it is excluded by the license, the data file or
//!   the resource key). This is the
//!   ["missing properties" rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#missing-properties).

use std::borrow::Cow;
use std::fmt;

use thiserror::Error;

/// The result type used across the pipeline crates.
pub type Result<T> = std::result::Result<T, Error>;

/// The reason a property is missing from a result set.
///
/// Used to build the explanatory message for [`Error::PropertyMissing`]. The
/// variants mirror the rows of the missing-property table in the
/// [properties specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#missing-properties).
/// Engines select the variant that matches their deployment (on-premise or
/// cloud) and the cause they detected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MissingPropertyReason {
    /// On-premise only. The property is not present in the loaded data file
    /// because the license and/or data file does not include it.
    DataFileUpgradeRequired,
    /// On-premise only. The property has been excluded by the engine
    /// configuration.
    PropertyExcludedFromConfig,
    /// Cloud only. The resource key does not grant access to any properties
    /// under the relevant product.
    ProductNotAccessibleWithResourceKey,
    /// Cloud only. The resource key does not grant access to this specific
    /// property.
    PropertyNotAccessibleWithResourceKey,
    /// The property is unknown to the element entirely, or the reason could not
    /// be determined.
    Unknown,
    /// Cloud only. The cloud request for this flow data failed, so the cloud
    /// engines had no response to populate the property from.
    CloudRequestFailed,
}

impl MissingPropertyReason {
    /// A short, human-readable explanation of the reason, suitable for
    /// inclusion in an error message.
    pub fn description(&self) -> &'static str {
        match self {
            MissingPropertyReason::DataFileUpgradeRequired => {
                "your license and/or data file does not include this property"
            }
            MissingPropertyReason::PropertyExcludedFromConfig => {
                "the property has been excluded when configuring the engine"
            }
            MissingPropertyReason::ProductNotAccessibleWithResourceKey => {
                "your resource key does not include access to any properties \
                 for this product"
            }
            MissingPropertyReason::PropertyNotAccessibleWithResourceKey => {
                "your resource key does not include access to this property"
            }
            MissingPropertyReason::Unknown => "the property is not available in the result set",
            MissingPropertyReason::CloudRequestFailed => {
                "an upstream cloud request failed, so no data was available to populate \
                 the property"
            }
        }
    }
}

impl fmt::Display for MissingPropertyReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
    }
}

/// Returned when a property value is present in the result set but the element
/// chose not to set it.
///
/// It carries a customizable message explaining why the value is not set, as
/// required by the
/// [null-values rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values).
///
/// It is deliberately a separate type from [`enum@Error`] so the dynamic property
/// bag ([`crate::ElementData::get`]) can return the narrowest possible error
/// without forcing callers to match on unrelated variants.
#[derive(Clone, PartialEq, Eq, Error)]
#[error("{}", crate::redact::redact(.message))]
pub struct NoValueError {
    /// The explanation of why no value is available.
    pub message: String,
}

impl NoValueError {
    /// Create a new [`NoValueError`] with the supplied explanatory message.
    pub fn new(message: impl Into<String>) -> Self {
        NoValueError {
            message: message.into(),
        }
    }
}

/// A single error recorded against a [`crate::FlowData`] instance during
/// processing.
///
/// The pipeline records one of these for each element that returns an error,
/// whether or not `suppress_process_exceptions` is enabled, and an element
/// that carries on past a failure can record one with
/// [`crate::FlowData::add_error`], per the
/// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#process-function).
#[derive(Debug, Error)]
#[error("error in element '{element_data_key}': {source}")]
pub struct FlowError {
    /// The data key of the [`crate::FlowElement`] that the error occurred in,
    /// or is related to.
    pub element_data_key: String,
    /// Whether the pipeline should re-throw this error when aggregating. This
    /// lets an element record an informational error without forcing
    /// propagation.
    pub should_throw: bool,
    /// The underlying error.
    pub source: Error,
}

impl FlowError {
    /// Create a new [`FlowError`] for the given element data key and error.
    /// `should_throw` defaults to `true`.
    pub fn new(element_data_key: impl Into<String>, source: Error) -> Self {
        FlowError {
            element_data_key: element_data_key.into(),
            should_throw: true,
            source,
        }
    }

    /// Create a new [`FlowError`] with an explicit `should_throw` flag.
    pub fn with_should_throw(
        element_data_key: impl Into<String>,
        source: Error,
        should_throw: bool,
    ) -> Self {
        FlowError {
            element_data_key: element_data_key.into(),
            should_throw,
            source,
        }
    }
}

/// The error type shared across the 51Degrees pipeline crates.
///
/// Variants correspond to the custom exception types named in the
/// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#custom-exceptionserrors).
/// The enum is `#[non_exhaustive]` so engine crates and future revisions can
/// add variants (for example a cloud-request error) without it being a breaking
/// change for downstream `match` expressions.
#[derive(Error)]
#[non_exhaustive]
pub enum Error {
    /// A property that an element declares it can populate was requested but is
    /// not present in the result set.
    ///
    /// Corresponds to the
    /// [missing-properties rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#missing-properties).
    #[error(
        "property '{property}' not found in data for element '{element_data_key}'. \
         This is because {reason}."
    )]
    PropertyMissing {
        /// The name of the property that was requested.
        property: String,
        /// The data key of the element that should have populated it.
        element_data_key: String,
        /// The reason the property is missing.
        reason: MissingPropertyReason,
    },

    /// A property value is present in the result set but the element chose not
    /// to set it.
    ///
    /// This wraps a [`NoValueError`] so it can travel through the [`enum@Error`]
    /// channel where a unified error type is required, while still being a
    /// conceptually distinct condition from [`Error::PropertyMissing`].
    #[error(transparent)]
    NoValue(#[from] NoValueError),

    /// Something in the supplied configuration is preventing the creation or
    /// execution of the pipeline.
    ///
    /// Thrown by pipelines, elements or their builders.
    #[error("pipeline configuration error: {}", crate::redact::redact(.message))]
    PipelineConfiguration {
        /// A description of what is wrong with the configuration.
        message: String,
    },

    /// An aggregate of one or more per-element errors that occurred during
    /// processing while exceptions were not suppressed.
    ///
    /// Thrown at the end of processing when `suppress_process_exceptions` is
    /// `false`. Only errors whose [`FlowError::should_throw`] is `true` are
    /// included. The message names each one, in brackets after the count, so
    /// the cause shows wherever the error is printed.
    #[error(
        "{} error(s) occurred during pipeline processing{}",
        .0.len(),
        describe_each(.0)
    )]
    Aggregate(Vec<FlowError>),

    /// A pipeline operation was attempted that requires processing to have
    /// completed, but the [`crate::FlowData`] has not been processed yet.
    ///
    /// This is the "user has done something wrong" case from the
    /// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#flow-data-and-derived-accessors).
    #[error("flow data has not been processed yet: {}", crate::redact::redact(.message))]
    NotProcessed {
        /// Detail of which operation required processing to have completed.
        message: String,
    },

    /// A request to the 51Degrees cloud service failed.
    ///
    /// Carries the HTTP status code, an optional retry-after hint in seconds
    /// parsed from the response, and the service or transport message. Raised
    /// by the cloud request engine.
    #[error("cloud request failed with status {status_code}: {}", crate::redact::redact(.message))]
    CloudRequest {
        /// The HTTP status code returned by the cloud service. Zero when the
        /// request did not complete, for example a connection failure.
        status_code: u16,
        /// The number of seconds to wait before retrying, when the service
        /// supplied a Retry-After hint.
        retry_after_seconds: Option<u64>,
        /// The error message from the cloud service, or a description of the
        /// transport failure.
        message: String,
    },

    /// A call into a native on-premise engine library failed.
    ///
    /// Raised across the FFI boundary by the on-premise device detection and IP
    /// intelligence engines when a native call returns a non-success status
    /// code or sets a native exception.
    #[error("native engine error ({status}): {}", crate::redact::redact(.message))]
    Native {
        /// The native status code, as its name or numeric value.
        status: String,
        /// The human-readable detail of the failure.
        message: String,
    },

    /// A failure in an element that none of the other variants describes,
    /// as .NET's `PipelineException`.
    ///
    /// A cloud engine records one, without failing processing, when the cloud
    /// request engine before it left no response.
    #[error("pipeline error: {}", crate::redact::redact(.message))]
    Pipeline {
        /// A description of the failure.
        message: String,
    },
}

/// Debug is written by hand rather than derived so that the free-text fields
/// go through [`crate::redact`] as well.
///
/// Display alone is not enough. `Result::unwrap` and `Result::expect` print the
/// `Debug` of the error, and that is how a resource key reached a test log, so
/// both of the ways an error can be turned into text have to be covered.
impl fmt::Debug for NoValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoValueError")
            .field("message", &crate::redact::redact(&self.message))
            .finish()
    }
}

/// Debug is written by hand for the same reason as [`NoValueError`], being that
/// `unwrap` and `expect` print `Debug` and a message can carry a credential.
/// The shape matches what the derive would print, so nothing a reader relies on
/// changes apart from the removed values.
impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PropertyMissing {
                property,
                element_data_key,
                reason,
            } => f
                .debug_struct("PropertyMissing")
                .field("property", property)
                .field("element_data_key", element_data_key)
                .field("reason", reason)
                .finish(),
            Error::NoValue(inner) => f.debug_tuple("NoValue").field(inner).finish(),
            Error::PipelineConfiguration { message } => f
                .debug_struct("PipelineConfiguration")
                .field("message", &crate::redact::redact(message))
                .finish(),
            Error::Aggregate(errors) => f.debug_tuple("Aggregate").field(errors).finish(),
            Error::NotProcessed { message } => f
                .debug_struct("NotProcessed")
                .field("message", &crate::redact::redact(message))
                .finish(),
            Error::CloudRequest {
                status_code,
                retry_after_seconds,
                message,
            } => f
                .debug_struct("CloudRequest")
                .field("status_code", status_code)
                .field("retry_after_seconds", retry_after_seconds)
                .field("message", &crate::redact::redact(message))
                .finish(),
            Error::Native { status, message } => f
                .debug_struct("Native")
                .field("status", status)
                .field("message", &crate::redact::redact(message))
                .finish(),
            Error::Pipeline { message } => f
                .debug_struct("Pipeline")
                .field("message", &crate::redact::redact(message))
                .finish(),
        }
    }
}

impl Error {
    /// Convenience constructor for a [`Error::PipelineConfiguration`] error.
    pub fn configuration(message: impl Into<String>) -> Self {
        Error::PipelineConfiguration {
            message: message.into(),
        }
    }

    /// The error's message without the words naming its kind, as .NET's
    /// `Exception.Message` and Java's `getMessage` give it, with credentials
    /// removed as they are from the display. The JSON builder writes this in
    /// `errors`.
    ///
    /// A missing property and an aggregate give their display, which is
    /// already the message.
    pub fn message(&self) -> Cow<'_, str> {
        match self {
            Error::NoValue(inner) => crate::redact::redact(&inner.message),
            Error::PipelineConfiguration { message }
            | Error::NotProcessed { message }
            | Error::CloudRequest { message, .. }
            | Error::Native { message, .. }
            | Error::Pipeline { message } => crate::redact::redact(message),
            other => Cow::Owned(other.to_string()),
        }
    }
}

/// Each error of an aggregate in brackets, for the aggregate's message. Every
/// error removes credentials from its own text.
fn describe_each(errors: &[FlowError]) -> String {
    errors.iter().map(|error| format!(" ({error})")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only key-shaped value written anywhere in this repository. It is not
    /// a real resource key and the service refuses it.
    const NOT_A_KEY: &str = "AQ-NOT-A-REAL-KEY-000000";

    fn cloud_error_carrying_a_key() -> Error {
        Error::CloudRequest {
            status_code: 400,
            retry_after_seconds: None,
            message: format!(
                "Cloud service at 'https://cloud.51degrees.com/api/v4/accessibleproperties\
                 ?resource={NOT_A_KEY}' returned status code '400' with content \
                 {{\"errors\":[\"'{NOT_A_KEY}' could not be read as a valid resource key.\"]}}"
            ),
        }
    }

    #[test]
    fn displaying_a_cloud_error_removes_the_key_and_keeps_the_reason() {
        let shown = cloud_error_carrying_a_key().to_string();
        assert!(!shown.contains(NOT_A_KEY), "the key survived: {shown}");
        assert!(shown.contains("status 400"), "the status was lost: {shown}");
        assert!(
            shown.contains("could not be read as a valid resource key"),
            "the reason was lost: {shown}"
        );
        assert!(
            shown.contains("accessibleproperties"),
            "the failing operation was lost: {shown}"
        );
    }

    #[test]
    fn debugging_a_cloud_error_removes_the_key() {
        // This is the path `unwrap` and `expect` take, which is how the key
        // reached a test log.
        let shown = format!("{:?}", cloud_error_carrying_a_key());
        assert!(!shown.contains(NOT_A_KEY), "the key survived: {shown}");
        assert!(
            shown.contains("status_code: 400"),
            "the status was lost: {shown}"
        );
    }

    /// A failing call, behind a function so the panic below is reached the way
    /// a caller reaches it rather than from a literal the compiler can see
    /// through. Only compiled where a panic unwinds, because the test below
    /// catches one.
    #[cfg(panic = "unwind")]
    fn a_failing_call() -> Result<()> {
        Err(cloud_error_carrying_a_key())
    }

    // wasm32-wasip1 compiles with panic = "abort", where a panic ends the whole
    // test process rather than being caught, so this test is compiled only for
    // the unwinding targets. The other tests in this module cover the same
    // redaction through Display and Debug, which is what the panic message is
    // built from, and they do run on wasm.
    #[cfg(panic = "unwind")]
    #[test]
    fn unwrapping_a_cloud_error_removes_the_key() {
        let panic = std::panic::catch_unwind(|| a_failing_call().unwrap()).unwrap_err();
        let shown = panic
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_else(|| "the panic payload was not a string".to_owned());
        assert!(!shown.contains(NOT_A_KEY), "the key survived: {shown}");
    }

    #[test]
    fn a_configuration_error_is_redacted() {
        let error =
            Error::configuration(format!("the endpoint '?resource={NOT_A_KEY}' is invalid"));
        assert!(!error.to_string().contains(NOT_A_KEY));
        assert!(!format!("{error:?}").contains(NOT_A_KEY));
        assert!(error.to_string().contains("is invalid"));
    }

    #[test]
    fn an_aggregate_redacts_the_errors_it_carries() {
        let aggregate =
            Error::Aggregate(vec![FlowError::new("cloud", cloud_error_carrying_a_key())]);
        assert!(!format!("{aggregate:?}").contains(NOT_A_KEY));
        assert!(!aggregate.to_string().contains(NOT_A_KEY));
        let flow = FlowError::new("cloud", cloud_error_carrying_a_key());
        assert!(!flow.to_string().contains(NOT_A_KEY));
        assert!(!format!("{flow:?}").contains(NOT_A_KEY));
    }

    #[test]
    fn an_aggregate_says_what_failed() {
        let aggregate = Error::Aggregate(vec![
            FlowError::new("cloud", cloud_error_carrying_a_key()),
            FlowError::new("device", Error::configuration("the engine failed")),
        ]);
        let shown = aggregate.to_string();
        assert!(
            shown.starts_with("2 error(s) occurred during pipeline processing"),
            "{shown}"
        );
        assert!(
            shown.contains("(error in element 'cloud': cloud request failed with status 400:"),
            "the first error is named: {shown}"
        );
        assert!(
            shown.contains(
                "(error in element 'device': pipeline configuration error: the engine failed)"
            ),
            "the second error is named: {shown}"
        );
    }

    #[test]
    fn a_pipeline_error_is_redacted() {
        let error = Error::Pipeline {
            message: format!("the element failed calling '?resource={NOT_A_KEY}'"),
        };
        assert!(!error.to_string().contains(NOT_A_KEY));
        assert!(!format!("{error:?}").contains(NOT_A_KEY));
        assert!(error
            .to_string()
            .starts_with("pipeline error: the element failed"));
    }

    #[test]
    fn a_failed_cloud_request_reads_as_dotnet_says() {
        let error = Error::PropertyMissing {
            property: "ismobile".to_owned(),
            element_data_key: "device".to_owned(),
            reason: MissingPropertyReason::CloudRequestFailed,
        };
        assert_eq!(
            error.to_string(),
            "property 'ismobile' not found in data for element 'device'. This is because \
             an upstream cloud request failed, so no data was available to populate the \
             property."
        );
    }

    #[test]
    fn a_no_value_error_is_redacted() {
        let error = NoValueError::new(format!("no value because '{NOT_A_KEY}' is refused"));
        assert!(!error.to_string().contains(NOT_A_KEY));
        assert!(!format!("{error:?}").contains(NOT_A_KEY));
        assert!(error.to_string().contains("is refused"));
    }

    #[test]
    fn a_message_leaves_out_the_kind_and_removes_the_key() {
        let error = Error::Pipeline {
            message: "carried on".to_owned(),
        };
        assert_eq!(error.to_string(), "pipeline error: carried on");
        assert_eq!(error.message(), "carried on");

        let cloud = cloud_error_carrying_a_key().message().into_owned();
        assert!(!cloud.contains(NOT_A_KEY), "the key survived: {cloud}");
        assert!(cloud.starts_with("Cloud service at "), "{cloud}");

        let native = Error::Native {
            status: "InvalidInput".to_owned(),
            message: "no address".to_owned(),
        };
        assert_eq!(native.message(), "no address");

        let missing = Error::PropertyMissing {
            property: "ismobile".to_owned(),
            element_data_key: "device".to_owned(),
            reason: MissingPropertyReason::PropertyNotAccessibleWithResourceKey,
        };
        assert_eq!(missing.message(), missing.to_string());
    }

    #[test]
    fn an_error_with_nothing_secret_reads_exactly_as_before() {
        let error = Error::PropertyMissing {
            property: "ismobile".to_owned(),
            element_data_key: "device".to_owned(),
            reason: MissingPropertyReason::PropertyNotAccessibleWithResourceKey,
        };
        assert_eq!(
            error.to_string(),
            "property 'ismobile' not found in data for element 'device'. This is because \
             your resource key does not include access to this property."
        );
    }
}
