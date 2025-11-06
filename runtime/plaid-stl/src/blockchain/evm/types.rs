use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct ChainId(u64);

impl FromStr for ChainId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let id: u64 = s.parse()?;
        Ok(ChainId(id))
    }
}

macro_rules! impl_from_uint {
    ($($t:ty),*) => {
        $(
            impl From<$t> for ChainId {
                fn from(id: $t) -> Self {
                    ChainId(id as u64)
                }
            }
        )*
    };
}

impl_from_uint!(u8, u16, u32, u64);

impl ChainId {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Deserialize, Serialize)]
pub struct GetTransactionRequest {
    /// The chain ID to query
    pub chain_id: ChainId,
    /// The transaction hash to look up
    pub hash: String,
}

/// Represents the error details in a failed JSON-RPC call.
#[derive(Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    /// Error code indicating the type of failure.
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Additional data related to the error.
    pub data: Option<String>,
}

/// Basic structure of a JSON-RPC response.
#[derive(Deserialize, Debug, Clone)]
pub struct BasicRpcResponse {
    /// Optional error details, if the call failed.
    pub error: Option<JsonRpcError>,
    /// Optional result, if the call was successful.
    pub result: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DetailedRpcResponse {
    /// Optional error details, if the call failed.
    pub error: Option<JsonRpcError>,
    /// Optional result, if the call was successful.
    pub result: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
pub enum BlockTag {
    /// The lowest numbered block the client has available
    Earliest,
    /// The most recent crypto-economically secure block, cannot be re-orged outside of manual intervention driven by community coordination
    Finalized,
    /// The most recent block in the canonical chain observed by the client,
    /// this block may be re-orged out of the canonical chain even under healthy/normal conditions
    Latest,
    /// A sample next block built by the client on top of `latest` and containing the set of transactions usually taken from local mempoo
    Pending,
    /// The most recent block that is safe from re-orgs under honest majority and certain synchronicity assumptions
    Safe,
    /// A specific block number
    Number(u64),
}

impl Display for BlockTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockTag::Earliest => write!(f, "earliest"),
            BlockTag::Finalized => write!(f, "finalized"),
            BlockTag::Latest => write!(f, "latest"),
            BlockTag::Pending => write!(f, "pending"),
            BlockTag::Safe => write!(f, "safe"),
            BlockTag::Number(n) => write!(f, "0x{n:x}"),
        }
    }
}

/// Representation of an Ethereum transaction returned by `eth_getTransactionByHash`
#[derive(Deserialize, Debug, Clone)]
pub struct Transaction {
    /// Hash of the block where this transaction was in. null when its pending.
    #[serde(rename = "blockHash")]
    pub block_hash: Option<String>,
    /// Block number where this transaction was in. null when its pending.
    #[serde(rename = "blockNumber")]
    pub block_number: Option<String>,
    /// Address of the sender
    pub from: String,
    /// Gas provided by the sender
    pub gas: String,
    /// Gas price provided by the sender in Wei
    #[serde(rename = "gasPrice")]
    pub gas_price: String,
    /// The maximum total fee per gas the sender is willing to pay (includes the network / base fee and miner / priority fee) in wei
    /// Only present for EIP-1559 transactions
    #[serde(rename = "maxFeePerGas")]
    pub max_fee_per_gas: Option<String>,
    /// Maximum fee per gas the sender is willing to pay to miners in wei
    /// Only present for EIP-1559 transactions
    #[serde(rename = "maxPriorityFeePerGas")]
    pub max_priority_fee_per_gas: Option<String>,
    /// The data sent along with the transaction.
    pub input: String,
    /// Address of the receiver. null when its a contract creation transaction
    pub to: Option<String>,
    /// Integer of the transaction index position in the block. null when its pending.
    #[serde(rename = "transactionIndex")]
    pub transaction_index: Option<String>,
    /// Value transferred in Wei
    pub value: String,
    /// The type of transaction
    pub r#type: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TransactionReceipt {}

/// Request structure for broadcasting a transaction.
#[derive(Deserialize, Serialize)]
pub struct SendRawTransactionRequest {
    /// The chain ID to send the transaction to
    pub chain_id: ChainId,
    /// The signed transaction data
    pub signed_tx: String,
}

/// Request structure for getting basic metadata about an address.
#[derive(Deserialize, Serialize)]
pub struct GetAddressMetadataRequest {
    /// The chain ID to query
    pub chain_id: ChainId,
    /// The address to look up
    pub address: String,
    /// The block tag to query against
    pub block_tag: BlockTag,
}

/// Request structure for making a call to a contract without creating a transaction.
#[derive(Serialize, Deserialize)]
pub struct EthCallRequest {
    /// The chain ID to query
    pub chain_id: ChainId,
    /// The address to which the call is directed
    pub to: String,
    /// The data payload for the call
    pub data: String,
    /// The block tag to query against
    pub block_tag: BlockTag,
}

#[derive(Serialize, Deserialize)]
pub struct EstimateGasRequest {
    /// The account the transaction is sent from
    pub from: String,
    /// The address the transaction is directed to
    /// If `None`, it indicates a contract creation transaction
    pub to: Option<String>,
    /// The value sent along with the transaction
    pub value: Option<String>,
    /// The data payload for the transaction
    pub data: Option<String>,
    /// The block tag to query against
    pub block_tag: BlockTag,
    /// The chain ID to query
    pub chain_id: ChainId,
}

impl EstimateGasRequest {
    /// Create a new builder for EstimateGasRequest
    pub fn builder(
        chain_id: impl Into<ChainId>,
        from: impl Display,
        block_tag: BlockTag,
    ) -> EstimateGasRequestBuilder {
        EstimateGasRequestBuilder::new(chain_id, from.to_string(), block_tag)
    }
}

/// Builder for `EstimateGasRequest`
pub struct EstimateGasRequestBuilder {
    chain_id: ChainId,
    from: String,
    to: Option<String>,
    value: Option<String>,
    data: Option<String>,
    block_tag: BlockTag,
}

impl EstimateGasRequestBuilder {
    fn new(chain_id: impl Into<ChainId>, from: impl Display, block_tag: BlockTag) -> Self {
        Self {
            chain_id: chain_id.into(),
            from: from.to_string(),
            to: None,
            value: None,
            data: None,
            block_tag,
        }
    }

    /// Set the destination address
    pub fn to(mut self, to: impl Into<String>) -> Self {
        self.to = Some(to.into());
        self
    }

    /// Set the value to send (in wei)
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set the transaction data payload
    pub fn data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Set the block tag to query against
    pub fn block_tag(mut self, block_tag: BlockTag) -> Self {
        self.block_tag = block_tag;
        self
    }

    /// Build the EstimateGasRequest
    pub fn build(self) -> EstimateGasRequest {
        EstimateGasRequest {
            chain_id: self.chain_id,
            from: self.from,
            to: self.to,
            value: self.value,
            data: self.data,
            block_tag: self.block_tag,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct GetGasPriceRequest {
    /// The chain ID to query
    pub chain_id: ChainId,
}
