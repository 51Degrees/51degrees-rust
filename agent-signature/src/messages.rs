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

//! The sentences a property with no value carries, so an absent detail never
//! reads as an answer.

/// No signature headers were in the request.
pub(crate) const NO_VALUE_DETAIL_ABSENT: &str = "No signature headers were present in the request";

/// The signature simply did not carry the detail.
pub(crate) const NO_VALUE_DETAIL_NOT_SENT: &str = "The signature did not carry this information";

/// The signature or its headers could not be read.
pub(crate) const NO_VALUE_DETAIL_MALFORMED: &str = "The signature headers could not be read";

/// No signature carried the Web Bot Auth tag.
pub(crate) const NO_VALUE_DETAIL_WRONG_TAG: &str =
    "No signature in the request was made for automated agent traffic";

/// The signature named no agent.
pub(crate) const NO_VALUE_DETAIL_NO_AGENT: &str =
    "The signature named no agent, so there was nowhere to fetch a key from";

/// The signature covered nothing tying it to the request.
pub(crate) const NO_VALUE_DETAIL_UNBOUND: &str =
    "The signature covered nothing that ties it to this request, so it was not checked.";

/// The agent carried its key set inline and inline key sets are off.
pub(crate) const NO_VALUE_DETAIL_INLINE_DIRECTORY: &str = "The agent sent its key with the \
    request rather than publishing it, which this element does not accept.";

/// The purpose, where the key directory was never read.
pub(crate) const NO_VALUE_PURPOSE_NOT_READ: &str =
    "The signature was not verified so the directory was not read";

/// The purpose, where the key directory was read and says nothing.
pub(crate) const NO_VALUE_PURPOSE_NOT_STATED: &str = "The agent does not say what its keys are for";

/// Any agent card property, where no card was found.
pub(crate) const NO_VALUE_NO_CARD: &str = "No agent card available";
