/*
 * Rosetta
 *
 * Build Once. Integrate Your Blockchain Everywhere.
 *
 * The version of the OpenAPI document: 1.4.13
 *
 * Generated by: https://openapi-generator.tech
 */

/// `CallRequest` : `CallRequest` is the input to the `/call` endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CallRequest {
    /// Method is some network-specific procedure call. This method could map to a network-specific
    /// RPC endpoint, a method in an SDK generated from a smart contract, or some hybrid of the
    /// two.  The implementation must define all available methods in the Allow object. However, it
    /// is up to the caller to determine which parameters to provide when invoking `/call`.
    #[serde(rename = "method")]
    pub method: String,
    /// Parameters is some network-specific argument for a method. It is up to the caller to
    /// determine which parameters to provide when invoking `/call`.
    #[serde(rename = "parameters")]
    pub parameters: serde_json::Value,
    #[serde(rename = "block_identifier", skip_serializing_if = "Option::is_none")]
    pub block_identifier: Option<crate::PartialBlockIdentifier>,
}

impl CallRequest {
    /// `CallRequest` is the input to the `/call` endpoint.
    #[must_use]
    pub const fn new(
        method: String,
        parameters: serde_json::Value,
        block_identifier: Option<crate::PartialBlockIdentifier>,
    ) -> Self {
        Self { method, parameters, block_identifier }
    }
}
