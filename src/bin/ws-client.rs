use {
    anyhow::Context,
    clap::{Parser, Subcommand, ValueEnum},
    futures::{sink::SinkExt, stream::StreamExt},
    indicatif::{ProgressBar, ProgressStyle},
    serde::Deserialize,
    serde_json::{json, Value},
    solana_account_decoder::{UiAccountEncoding, UiDataSliceConfig},
    solana_client::{
        nonblocking::pubsub_client::PubsubClient,
        rpc_config::{
            RpcAccountInfoConfig, RpcBlockSubscribeConfig, RpcBlockSubscribeFilter,
            RpcProgramAccountsConfig, RpcSignatureSubscribeConfig, RpcTransactionLogsConfig,
            RpcTransactionLogsFilter,
        },
        rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType},
    },
    solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature},
    solana_transaction_status::{TransactionDetails, UiTransactionEncoding},
    std::{fmt, str::FromStr},
    tokio::net::TcpStream,
    tokio_tungstenite::{
        connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
    },
    tracing::info,
};

#[derive(Debug, Clone, Parser)]
struct Args {
    /// WebSocket endpoint
    #[clap(short, long, default_value_t = String::from("ws://127.0.0.1:8000/"))]
    endpoint: String,

    #[command(subcommand)]
    action: ArgsAction,
}

impl Args {
    fn parse_data_slice(data_slice: &Option<String>) -> anyhow::Result<Option<UiDataSliceConfig>> {
        Ok(if let Some(data_slice) = data_slice {
            match data_slice.split_once(',') {
                Some((offset, length)) => match (offset.parse(), length.parse()) {
                    (Ok(offset), Ok(length)) => Some(UiDataSliceConfig { offset, length }),
                    _ => anyhow::bail!("invalid data_slice: {data_slice}"),
                },
                _ => anyhow::bail!("invalid data_slice: {data_slice}"),
            }
        } else {
            None
        })
    }
}

#[derive(Debug, Clone, Subcommand)]
enum ArgsAction {
    Subscribe {
        /// Type of subscription
        #[command(subcommand)]
        action: SubscribeAction,
        /// Commitment level of subscritpion
        #[clap(short, long, default_value_t = SubscribeCommitment::default())]
        commitment: SubscribeCommitment,
        /// Show only progress bar with received messages
        #[clap(long, default_value_t = false)]
        only_counter: bool,
    },
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum SubscribeCommitment {
    Processed,
    Confirmed,
    #[default]
    Finalized,
}

impl fmt::Debug for SubscribeCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Processed => write!(f, "processed"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Finalized => write!(f, "finalized"),
        }
    }
}

impl fmt::Display for SubscribeCommitment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl From<SubscribeCommitment> for CommitmentConfig {
    fn from(commitment: SubscribeCommitment) -> Self {
        match commitment {
            SubscribeCommitment::Processed => Self::processed(),
            SubscribeCommitment::Confirmed => Self::confirmed(),
            SubscribeCommitment::Finalized => Self::finalized(),
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
enum SubscribeAction {
    // STABLE
    /// Subscribe on account updates
    Account {
        /// Account key
        #[clap(short, long)]
        pubkey: String,
        /// Encoding format
        #[clap(long, short)]
        encoding: Option<SubscribeUiAccountEncoding>,
        /// Apply slice to data in updated accounts, format: `offset,length`
        #[clap(long, short)]
        data_slice: Option<String>,
    },
    /// Subscribe on transactions log updates
    Logs {
        /// All transactions
        #[clap(long, default_value_t = false)]
        all: bool,
        /// All transactions with votes
        #[clap(long, default_value_t = false)]
        all_with_votes: bool,
        /// Only transactions with mentions
        #[clap(long)]
        mentions: Vec<String>,
    },
    /// Subscribe on accounts updates owned by program
    Program {
        /// Program account key
        #[clap(short, long)]
        pubkey: String,
        /// Filter by data size
        #[clap(long)]
        filter_data_size: Vec<u64>,
        /// Filter by memcmp, format: `offset,data in base58`
        #[clap(long)]
        filter_memcmp: Vec<String>,
        /// Encoding format
        #[clap(long, short)]
        encoding: Option<SubscribeUiAccountEncoding>,
        /// Apply slice to data in updated accounts, format: `offset,length`
        #[clap(long, short)]
        data_slice: Option<String>,
    },
    /// Subscribe on transaction confirmation events
    Signature {
        /// Transaction signature
        #[clap(short, long)]
        signature: String,
    },
    /// Subscribe on slot updates
    Slot,

    // UNSTABLE
    /// Subscribe on block updates
    Block {
        /// Program account key
        #[clap(short, long)]
        pubkey: Option<String>,
        /// Encoding format
        #[clap(long, short)]
        encoding: Option<SubscribeUiTransactionEncoding>,
        /// Transaction details
        #[clap(long, short)]
        transaction_details: Option<SubscribeTransactionDetails>,
        /// Show rewards
        #[clap(long, short)]
        show_rewards: Option<bool>,
        /// Maximum supported transaction version
        #[clap(long, short)]
        max_supported_transaction_version: Option<u8>,
    },
    // Subscribe on ?
    // Root {}
    // Subscribe on different updates of slot
    // SlotUpdate {}
    // Subscribe on ?
    // Vote {}

    // EXPERIMENTAL
    Transaction {
        /// Include vote transactions
        #[clap(long)]
        vote: Option<bool>,
        /// Include failed transactions
        #[clap(long)]
        failed: Option<bool>,
        /// Filter transactions by signature
        #[clap(long)]
        signature: Option<String>,
        /// Transaction should include any of these accounts
        #[clap(long)]
        account_include: Vec<String>,
        /// Transaction should not contain any of these accounts
        #[clap(long)]
        account_exclude: Vec<String>,
        /// Transaction should contain all these accounts
        #[clap(long)]
        account_required: Vec<String>,
        /// Encoding format
        #[clap(long, short)]
        encoding: Option<SubscribeUiTransactionEncoding>,
        /// Transaction details
        #[clap(long, short)]
        transaction_details: Option<SubscribeTransactionDetails>,
        /// Show rewards
        #[clap(long, short)]
        show_rewards: Option<bool>,
        /// Maximum supported transaction version
        #[clap(long, short)]
        max_supported_transaction_version: Option<u8>,
    },
    TransactionDeprecated {
        /// Include vote transactions
        #[clap(long)]
        vote: Option<bool>,
        /// Include failed transactions
        #[clap(long)]
        failed: Option<bool>,
        /// Transaction should include any of these accounts
        #[clap(long)]
        mentions: Vec<String>,
        /// Transaction should not contain any of these accounts
        #[clap(long)]
        exclude: Vec<String>,
        /// Transaction should contain all these accounts
        #[clap(long)]
        required: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SubscribeUiAccountEncoding {
    Binary,
    Base58,
    Base64,
    JsonParsed,
    Base64Zstd,
}

impl From<SubscribeUiAccountEncoding> for UiAccountEncoding {
    fn from(encoding: SubscribeUiAccountEncoding) -> Self {
        match encoding {
            SubscribeUiAccountEncoding::Binary => Self::Binary,
            SubscribeUiAccountEncoding::Base58 => Self::Base58,
            SubscribeUiAccountEncoding::Base64 => Self::Base64,
            SubscribeUiAccountEncoding::JsonParsed => Self::JsonParsed,
            SubscribeUiAccountEncoding::Base64Zstd => Self::Base64Zstd,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SubscribeUiTransactionEncoding {
    Binary,
    Base64,
    Base58,
    Json,
    JsonParsed,
}

impl From<SubscribeUiTransactionEncoding> for UiTransactionEncoding {
    fn from(encoding: SubscribeUiTransactionEncoding) -> Self {
        match encoding {
            SubscribeUiTransactionEncoding::Binary => Self::Binary,
            SubscribeUiTransactionEncoding::Base64 => Self::Base64,
            SubscribeUiTransactionEncoding::Base58 => Self::Base58,
            SubscribeUiTransactionEncoding::Json => Self::Json,
            SubscribeUiTransactionEncoding::JsonParsed => Self::JsonParsed,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SubscribeTransactionDetails {
    Full,
    Signatures,
    None,
    Accounts,
}

impl From<SubscribeTransactionDetails> for TransactionDetails {
    fn from(encoding: SubscribeTransactionDetails) -> Self {
        match encoding {
            SubscribeTransactionDetails::Full => TransactionDetails::Full,
            SubscribeTransactionDetails::Signatures => TransactionDetails::Signatures,
            SubscribeTransactionDetails::None => TransactionDetails::None,
            SubscribeTransactionDetails::Accounts => TransactionDetails::Accounts,
        }
    }
}

async fn subscribe_experimental(
    endpoint: &str,
    name: &str,
    params: Value,
) -> anyhow::Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let (mut stream, _) = connect_async(endpoint).await?;
    stream
        .send(Message::Text(
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": format!("{name}Subscribe"),
                "params": params,
            })
            .to_string(),
        ))
        .await?;

    match stream.next().await {
        Some(Ok(Message::Text(data))) => {
            #[derive(Deserialize)]
            struct Response {
                #[allow(dead_code)]
                result: usize,
            }

            let _ = serde_json::from_str::<Response>(&data)?;
            Ok(stream)
        }
        Some(Ok(_)) => anyhow::bail!("invalid message type"),
        Some(Err(error)) => anyhow::bail!(error),
        None => anyhow::bail!("no messages"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    whirligig_client::tracer_init()?;

    let args = Args::parse();
    let client = PubsubClient::new(&args.endpoint).await.unwrap();

    match args.action {
        ArgsAction::Subscribe {
            action,
            commitment,
            only_counter,
        } => {
            let pb = only_counter
                .then(|| {
                    let pb = ProgressBar::new(u64::MAX);
                    pb.set_style(ProgressStyle::with_template(
                        "{spinner:.green} +{pos} messages",
                    )?);
                    Ok::<_, anyhow::Error>(pb)
                })
                .transpose()?;
            let on_new_item = |f: &dyn Fn()| {
                if let Some(pb) = &pb {
                    pb.inc(1)
                } else {
                    f();
                }
            };

            match action {
                // STABLE
                SubscribeAction::Account {
                    pubkey,
                    encoding,
                    data_slice,
                } => {
                    let pubkey = Pubkey::from_str(&pubkey)
                        .with_context(|| format!("invalid pubkey: {pubkey}"))?;

                    let (mut stream, _unsubscribe) = client
                        .account_subscribe(
                            &pubkey,
                            Some(RpcAccountInfoConfig {
                                encoding: encoding.map(Into::into),
                                data_slice: Args::parse_data_slice(&data_slice)?,
                                commitment: Some(commitment.into()),
                                min_context_slot: None,
                            }),
                        )
                        .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("account, new item: {item:?}"));
                    }
                }
                SubscribeAction::Logs {
                    all,
                    all_with_votes,
                    mentions,
                } => {
                    let filter = match (all, all_with_votes, !mentions.is_empty()) {
                        (true, false, false) => RpcTransactionLogsFilter::All,
                        (false, true, false) => RpcTransactionLogsFilter::AllWithVotes,
                        (false, false, true) => RpcTransactionLogsFilter::Mentions(mentions),
                        _ => anyhow::bail!(
                            "conflicts between `all`, `all-with-votes` and `mentions`"
                        ),
                    };

                    let (mut stream, _unsubscribe) = client
                        .logs_subscribe(
                            filter,
                            RpcTransactionLogsConfig {
                                commitment: Some(commitment.into()),
                            },
                        )
                        .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("logs, new item: {item:?}"));
                    }
                }
                SubscribeAction::Program {
                    pubkey,
                    filter_data_size,
                    filter_memcmp,
                    encoding,
                    data_slice,
                } => {
                    let pubkey = Pubkey::from_str(&pubkey)
                        .with_context(|| format!("invalid pubkey: {pubkey}"))?;

                    let mut filters = vec![];
                    for data_size in filter_data_size {
                        filters.push(RpcFilterType::DataSize(data_size));
                    }
                    for memcmp in filter_memcmp {
                        match memcmp.split_once(',') {
                            Some((offset, data)) => {
                                filters.push(RpcFilterType::Memcmp(Memcmp::new(
                                    offset.parse().with_context(|| {
                                        format!("invalid offset in memcmp: {offset}")
                                    })?,
                                    MemcmpEncodedBytes::Base58(data.to_owned()),
                                )))
                            }
                            _ => anyhow::bail!("invalid memcmp: {memcmp}"),
                        }
                    }

                    let (mut stream, _unsubscribe) = client
                        .program_subscribe(
                            &pubkey,
                            Some(RpcProgramAccountsConfig {
                                filters: Some(filters),
                                account_config: RpcAccountInfoConfig {
                                    encoding: encoding.map(|e| e.into()),
                                    data_slice: Args::parse_data_slice(&data_slice)?,
                                    commitment: Some(commitment.into()),
                                    min_context_slot: None,
                                },
                                with_context: None,
                            }),
                        )
                        .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("program, new item: {item:?}"));
                    }
                }
                SubscribeAction::Signature { signature } => {
                    let signature = Signature::from_str(&signature)
                        .with_context(|| format!("invalid signature: {signature}"))?;

                    let (mut stream, _unsubscribe) = client
                        .signature_subscribe(
                            &signature,
                            Some(RpcSignatureSubscribeConfig {
                                commitment: Some(commitment.into()),
                                enable_received_notification: None,
                            }),
                        )
                        .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("signature, new item: {item:?}"));
                    }
                }
                SubscribeAction::Slot => {
                    let (mut stream, _unsubscribe) = client.slot_subscribe().await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("slot, new item: {item:?}"));
                    }
                }
                // UNSTABLE
                SubscribeAction::Block {
                    pubkey,
                    encoding,
                    transaction_details,
                    show_rewards,
                    max_supported_transaction_version,
                } => {
                    let filter = if let Some(pubkey) = pubkey {
                        RpcBlockSubscribeFilter::MentionsAccountOrProgram(
                            Pubkey::from_str(&pubkey)
                                .with_context(|| format!("invalid pubkey: {pubkey}"))?
                                .to_string(),
                        )
                    } else {
                        RpcBlockSubscribeFilter::All
                    };

                    let (mut stream, _unsubscribe) = client
                        .block_subscribe(
                            filter,
                            Some(RpcBlockSubscribeConfig {
                                commitment: Some(commitment.into()),
                                encoding: encoding.map(Into::into),
                                transaction_details: transaction_details.map(Into::into),
                                show_rewards,
                                max_supported_transaction_version,
                            }),
                        )
                        .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("block, new item: {item:?}"));
                    }
                }
                // EXPERIMENTAL
                SubscribeAction::Transaction {
                    vote,
                    failed,
                    signature,
                    account_include,
                    account_exclude,
                    account_required,
                    encoding,
                    transaction_details,
                    show_rewards,
                    max_supported_transaction_version,
                } => {
                    let mut stream = subscribe_experimental(
                        &args.endpoint,
                        "transaction",
                        json!([{
                            "vote": vote,
                            "failed": failed,
                            "signature": signature,
                            "accounts": {
                                "include": account_include,
                                "exclude": account_exclude,
                                "required": account_required,
                            }
                        }, {
                            "commitment": CommitmentConfig::from(commitment),
                            "encoding": encoding.map(UiTransactionEncoding::from),
                            "transactionDetails": transaction_details.map(TransactionDetails::from),
                            "showRewards": show_rewards,
                            "maxSupportedTransactionVersion": max_supported_transaction_version,
                        }]),
                    )
                    .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("transaction, new item: {item:?}"));
                    }
                }
                SubscribeAction::TransactionDeprecated {
                    vote,
                    failed,
                    mentions,
                    exclude,
                    required,
                } => {
                    let mut stream = subscribe_experimental(
                        &args.endpoint,
                        "transaction",
                        json!([{
                            "vote": vote,
                            "failed": failed,
                            "include": mentions,
                            "exclude": exclude,
                            "required": required,
                        }, {
                            "commitment": CommitmentConfig::from(commitment),
                        }]),
                    )
                    .await?;
                    while let Some(item) = stream.next().await {
                        on_new_item(&|| info!("transaction, new item: {item:?}"));
                    }
                }
            }
        }
    }

    Ok(())
}
