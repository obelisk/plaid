use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum Cluster {
    Mainnet,
    Devnet,
    Testnet,
}

impl Display for Cluster {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Cluster::Mainnet => "mainnet",
            Cluster::Devnet => "devnet",
            Cluster::Testnet => "testnet",
        })
    }
}

impl FromStr for Cluster {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "mainnet" => Ok(Cluster::Mainnet),
            "devnet" => Ok(Cluster::Devnet),
            "testnet" => Ok(Cluster::Testnet),
            other => Err(format!("unknown Solana cluster: {other}")),
        }
    }
}

/// A base58-encoded Solana account address (a 32-byte public key). Unvalidated
/// (the newtype is just to disambiguate with other strings: it's the caller's
/// responsibility to encode as 32 byte b58).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnvalidatedPubkey(String);

impl UnvalidatedPubkey {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for UnvalidatedPubkey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for UnvalidatedPubkey {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Display for UnvalidatedPubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for UnvalidatedPubkey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<UnvalidatedPubkey> for String {
    fn from(p: UnvalidatedPubkey) -> String {
        p.0
    }
}

/// A base58-encoded Solana transaction signature (64 bytes).
///
/// The signature analog of [`Pubkey`]: a disambiguation-only newtype that performs
/// **no validation**. See [`Pubkey`] for the rationale.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnvalidatedSignature(String);

impl UnvalidatedSignature {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for UnvalidatedSignature {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for UnvalidatedSignature {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Display for UnvalidatedSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for UnvalidatedSignature {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<UnvalidatedSignature> for String {
    fn from(s: UnvalidatedSignature) -> String {
        s.0
    }
}

/// A JSON-RPC error object, as returned by Solana nodes.
#[derive(Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    /// Error code indicating the type of failure.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Additional, method-specific error data (shape varies).
    pub data: Option<serde_json::Value>,
}

/// A Solana JSON-RPC response envelope.
///
/// Result shapes vary widely across methods, so `result` is surfaced as a raw
/// [`serde_json::Value`]. Use [`super::parse_rpc_response`] to extract a typed result.
#[derive(Deserialize, Debug, Clone)]
pub struct SolanaRpcResponse {
    /// Present if the call failed.
    pub error: Option<JsonRpcError>,
    /// Present if the call succeeded.
    pub result: Option<serde_json::Value>,
}

/// Submits a fully-signed transaction to a cluster.
///
/// Mirrors the EVM `SendRawTransactionRequest`: the caller is responsible for
/// building, signing, and encoding the transaction; the host simply relays it
/// to the node's `sendTransaction` RPC method.
#[derive(Serialize, Deserialize)]
pub struct SendTransactionRequest<'a> {
    /// Target cluster.
    pub cluster: Cluster,
    /// Base64-encoded, fully-signed transaction.
    pub transaction: Cow<'a, str>,
}

/// A request that only needs to select a cluster (no further parameters).
///
/// Shared by the no-argument read methods (`getSlot`, `getLatestBlockhash`,
/// `getTransactionCount`).
#[derive(Serialize, Deserialize)]
pub struct ClusterRequest {
    pub cluster: Cluster,
}

/// A request targeting a single account, identified by its base58 pubkey.
///
/// Shared by `getBalance` and `getAccountInfo` (mirrors EVM's
/// `GetAddressMetadataRequest`).
#[derive(Serialize, Deserialize)]
pub struct PubkeyRequest<'a> {
    pub cluster: Cluster,
    /// Account (or mint) address.
    pub pubkey: Cow<'a, UnvalidatedPubkey>,
}

/// Looks up a confirmed transaction by its signature.
#[derive(Serialize, Deserialize)]
pub struct GetTransactionRequest<'a> {
    pub cluster: Cluster,
    /// Transaction signature.
    pub signature: Cow<'a, UnvalidatedSignature>,
}

/// Looks up the processing status of a batch of transaction signatures.
#[derive(Serialize, Deserialize)]
pub struct GetSignatureStatusesRequest<'a> {
    pub cluster: Cluster,
    /// Transaction signatures.
    pub signatures: Cow<'a, [UnvalidatedSignature]>,
    /// Search the full transaction history rather than only the recent status
    /// cache. The history scan is expensive; leave `false` (the default) for the
    /// common "is this recent signature confirmed?" check, and set `true` only
    /// when looking up signatures old enough to have left cache.
    #[serde(default)]
    pub search_transaction_history: bool,
}

/// Fetches a confirmed block by slot number.
#[derive(Serialize, Deserialize)]
pub struct GetBlockRequest {
    pub cluster: Cluster,
    pub slot: u64,
}

/// Reads multiple accounts in a single call (batched `getAccountInfo`).
#[derive(Serialize, Deserialize)]
pub struct GetMultipleAccountsRequest<'a> {
    pub cluster: Cluster,
    /// Account addresses.
    pub pubkeys: Cow<'a, [UnvalidatedPubkey]>,
}

/// A `memcmp` filter for `getProgramAccounts`: keeps only accounts whose data,
/// starting at `offset`, equals `bytes`.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Memcmp {
    /// Byte offset into the account data at which to start the comparison.
    pub offset: u64,
    /// The bytes to match, encoded per `encoding`.
    pub bytes: String,
    /// Encoding of `bytes`: `"base58"` or `"base64"`. The RPC default is base58.
    #[serde(default)]
    pub encoding: Option<String>,
}

/// Limits each returned account's data to a `length`-byte window starting at
/// `offset` (the `getProgramAccounts` / `getAccountInfo` `dataSlice` option).
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct DataSlice {
    pub offset: u64,
    pub length: u64,
}

/// Optional filters and projection for [`GetProgramAccountsRequest`].
///
/// An unfiltered scan returns *every* account a program owns, which for real
/// programs is large enough to exceed the host return buffer or be rejected by
/// RPC providers. Scope the query with `data_size` / `memcmp`, and trim each
/// result with `data_slice`. [`Default`] (all unset) reproduces the full scan.
#[derive(Serialize, Deserialize, Default)]
pub struct ProgramAccountsFilters {
    /// Keep only accounts whose data is exactly this many bytes.
    #[serde(default)]
    pub data_size: Option<u64>,
    /// `memcmp` byte-match filters; multiple are ANDed together.
    #[serde(default)]
    pub memcmp: Vec<Memcmp>,
    /// Return only a slice of each matched account's data.
    #[serde(default)]
    pub data_slice: Option<DataSlice>,
}

/// Enumerates all accounts owned by a program.
///
/// The fundamental Solana state query; has no EVM equivalent (EVM uses events/logs).
#[derive(Serialize, Deserialize)]
pub struct GetProgramAccountsRequest<'a> {
    pub cluster: Cluster,
    /// Program id whose accounts to enumerate.
    pub program_id: Cow<'a, UnvalidatedPubkey>,
    /// Filters/projection narrowing the scan. Defaults to an unfiltered scan.
    #[serde(default)]
    pub filters: ProgramAccountsFilters,
}

/// Lists the SPL token accounts owned by a wallet.
///
/// Exactly one of `mint` or `program_id` selects the filter; if neither is set,
/// the host defaults to the SPL Token program.
#[derive(Serialize, Deserialize)]
pub struct GetTokenAccountsByOwnerRequest<'a> {
    pub cluster: Cluster,
    /// Owner wallet address.
    pub owner: Cow<'a, UnvalidatedPubkey>,
    /// Filter to a specific mint.
    pub mint: Option<Cow<'a, UnvalidatedPubkey>>,
    /// Filter to a specific token program.
    pub program_id: Option<Cow<'a, UnvalidatedPubkey>>,
}

/// Returns the lamports required for an account of `data_length` bytes to be rent-exempt.
///
/// Solana-specific (rent model); needed before creating accounts.
#[derive(Serialize, Deserialize)]
pub struct GetMinimumBalanceForRentExemptionRequest {
    pub cluster: Cluster,
    pub data_length: u64,
}

/// Returns the fee the cluster would charge for a serialized message.
#[derive(Serialize, Deserialize)]
pub struct GetFeeForMessageRequest<'a> {
    pub cluster: Cluster,
    /// Base64-encoded transaction message.
    pub message: Cow<'a, str>,
}

/// Returns a list of recent prioritization fees, optionally scoped to accounts.
#[derive(Serialize, Deserialize)]
pub struct GetRecentPrioritizationFeesRequest<'a> {
    pub cluster: Cluster,
    /// Account addresses to scope the query (may be empty).
    #[serde(default)]
    pub addresses: Cow<'a, [UnvalidatedPubkey]>,
}

/// Returns signatures for transactions involving an address, most recent first.
///
/// The per-address transaction history Solana offers in place of EVM log-scraping.
#[derive(Serialize, Deserialize)]
pub struct GetSignaturesForAddressRequest<'a> {
    pub cluster: Cluster,
    /// Account address.
    pub address: Cow<'a, UnvalidatedPubkey>,
    /// Maximum number of signatures to return (RPC default is 1000).
    pub limit: Option<u64>,
}
