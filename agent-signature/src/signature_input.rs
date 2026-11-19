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

//! Pairs each `Signature-Input` member with the `Signature` member of the
//! same label.

use crate::structured_field::{
    parameter, serialize_member, string_parameter, BareItem, Dictionary, Item, Member,
};

/// One signature from the request, with the parameters it was made with.
#[derive(Debug, Clone)]
pub(crate) struct SignatureCandidate {
    /// The components the signature covers, in the order the signer listed
    /// them.
    pub(crate) covered_components: Vec<Item>,
    /// The strict serialisation of the `Signature-Input` member, which is the
    /// last line of the signature base.
    pub(crate) signature_params: String,
    pub(crate) signature: Vec<u8>,
    pub(crate) created: Option<i64>,
    pub(crate) expires: Option<i64>,
    pub(crate) key_id: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) nonce: Option<String>,
    pub(crate) algorithm: Option<String>,
}

impl SignatureCandidate {
    /// Pair the two headers. Every `Signature-Input` member must be an inner
    /// list with a byte sequence under the same label in `Signature`, and the
    /// two must hold the same number of members, or neither can be read.
    pub(crate) fn build_all(
        input: &Dictionary,
        signature: &Dictionary,
    ) -> Option<Vec<SignatureCandidate>> {
        if input.members().is_empty() || input.members().len() != signature.members().len() {
            return None;
        }
        input
            .members()
            .iter()
            .map(|(label, member)| {
                let Member::InnerList(components, parameters) = member else {
                    return None;
                };
                let Some(Member::Item(Item {
                    value: BareItem::Bytes(bytes),
                    ..
                })) = signature.get(label)
                else {
                    return None;
                };
                Some(SignatureCandidate {
                    covered_components: components.clone(),
                    signature_params: serialize_member(member),
                    signature: bytes.clone(),
                    created: integer_parameter(parameters, "created"),
                    expires: integer_parameter(parameters, "expires"),
                    key_id: string_parameter(parameters, "keyid").map(str::to_owned),
                    tag: string_parameter(parameters, "tag").map(str::to_owned),
                    nonce: string_parameter(parameters, "nonce").map(str::to_owned),
                    algorithm: string_parameter(parameters, "alg").map(str::to_owned),
                })
            })
            .collect()
    }
}

fn integer_parameter(parameters: &[(String, BareItem)], name: &str) -> Option<i64> {
    match parameter(parameters, name)? {
        BareItem::Integer(value) => Some(*value),
        _ => None,
    }
}
