use {
    anyhow::Context,
    clap::Parser,
    futures::stream::StreamExt,
    indicatif::{ProgressBar, ProgressStyle},
    solana_client::{
        nonblocking::pubsub_client::PubsubClient,
        rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter},
    },
    solana_sdk::pubkey::Pubkey,
    std::{borrow::Cow, str::FromStr, sync::Arc},
    tokio::{
        fs,
        task::JoinSet,
        time::{sleep, Duration},
    },
};

const ACCOUNTS: &str = "";
const OWNERS: &str = "
11111111111111111111111111111111
26LYr2NRPprQ7aq6HTyAvrWxhouH8c9KLv1KumtRTJu2
2V7t5NaKY7aGkwytCWQgvUYZfEr9XMwNChhJEakTExk6
5ALDzwcRJfSyGdGyhP3kP628aqBNHZzLuVww7o9kdspe
5JQ8Mhdp2wv3HWcfjq9Ts8kwzCAeBADFBDAgBznzRsE4
675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8
7Sur3cy2efJGv8Qomn35p5k6HqMMcE8juWMBX8sCc96r
8RMnV1eD55iqUFJLMguPkYBkq8DCtx81XcmAja93LvRR
9aiGb2qTGB7xxrEWRrHtzgzBYTfq4y51hQGHrYxxJWna
9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin
ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
cndy3Z4yapfJBmL3ShUp5exZKqR3z33thTzeNMm2gRZ
ComputeBudget111111111111111111111111111111
CuieVDEDtLo7FypA9SbLM9saXFdb1dsshEkyErMqkRQq
D3z8BLmMnPD1LaKwCkyCisM7iDyw9PsXXmvatUwjCuqT
DtmE9D2CSB4L5D6A15mraeEjrGMm6auWVzgaD8hK2tZM
DzpARSggzEgNc2HLdrMEguSf9KGPh5RG1NMBqNnF8ZuU
F42dQ3SMssashRsA4SRfwJxFkGKV1bE3TcmpkagX8vvX
FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH
FZsgu4Gv9fn1iUm5v7iW3p9joX9HJcmxgXdRCqCGxpfE
GCQ2KPaxKeweMdHcJtfdFd88o1mQvntA1oPSBkSfJwQp
GDDMwNyyx8uB6zrqwBFHjLLG3TBYk2F8Az4yrQC5RzMp
GVXRSBjFk6e6J3NbVPXohDJetcTjaeeuykUpbQF8UoMU
H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4AQJEG
HfeFy4G9r77iyeXdbfNJjYw4z3NPEKDL6YQh3JzJ9s9f
JBu1AL4obBcCMqKBBxhpWCNUt136ijcuMZLFvTP7iWdB
JUP2jxvXaqu7NQY1GmNF4m1vodw12LVXYxbFL2uJvfo
KeccakSecp256k11111111111111111111111111111
MEisE1HzehtrDpAAT8PnLHjpSSkRYakotTuJRPjTpo8
Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo
MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr
mv3ekLzLbnVPNxjSKvqBpU3ZeZXPQdEC3bp5MDEBG68
PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY
So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo
SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f
TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
Vote111111111111111111111111111111111111111
whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc
Y2akr3bXHRsqyP1QJtbm9G9N88ZV4t1KfaFeDzKRTfr
ZETAxsqBRek56DhiGXrn75yj2NHU3aYUnxvHXpkf3aD
Zo1ggzTUKMY5bYnDvT5mtVeZxzf2FaLTbKkmvGUhUQk
";

#[derive(Debug, Clone, Parser)]
struct Args {
    /// WebSocket endpoint
    #[clap(short, long, default_value_t = String::from("ws://127.0.0.1:8000/streams"))]
    endpoint: String,

    /// Path to file with accounts Pubkeys
    #[clap(long, short)]
    accounts: Option<String>,

    /// Path to file with owners Pubkeys, by default some most used programs are used
    #[clap(long, short)]
    owners: Option<String>,

    /// Additionally subscribe on transactions with both `accounts` and `owners`
    #[clap(long, short, default_value_t = true)]
    transactions: bool,
}

async fn load_pubkeys(path: Option<&str>, default: &'static str) -> anyhow::Result<Vec<Pubkey>> {
    let data = if let Some(path) = path {
        Cow::Owned(
            fs::read_to_string(path)
                .await
                .with_context(|| format!("failed to read file: {path:?}"))?,
        )
    } else {
        Cow::Borrowed(default)
    };

    data.split('\n')
        .filter_map(|mut line| {
            line = line.trim();
            if line.is_empty() {
                None
            } else {
                Some(Pubkey::from_str(line))
            }
        })
        .collect::<Result<Vec<Pubkey>, _>>()
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    whirligig_client::tracer_init()?;

    let args = Args::parse();

    let accounts = load_pubkeys(args.accounts.as_deref(), ACCOUNTS).await?;
    let owners = load_pubkeys(args.owners.as_deref(), OWNERS).await?;

    let mut tasks = JoinSet::new();
    let pb = Arc::new(ProgressBar::new(u64::MAX));
    pb.set_style(ProgressStyle::with_template(
        "{spinner:.green} +{pos} messages",
    )?);

    #[allow(clippy::unnecessary_to_owned)]
    for pubkey in accounts.iter().cloned() {
        let endpoint = args.endpoint.clone();
        let pb = Arc::clone(&pb);
        tasks.spawn(async move {
            let client = PubsubClient::new(&endpoint).await?;
            let (mut stream, _unsubscribe) = client.account_subscribe(&pubkey, None).await?;
            pb.println(format!("subscribe on accout updates: {pubkey}"));
            while stream.next().await.is_some() {
                pb.inc(1);
            }
            anyhow::bail!("stream finished");
        });
    }

    #[allow(clippy::unnecessary_to_owned)]
    for pubkey in owners.iter().cloned() {
        let endpoint = args.endpoint.clone();
        let pb = Arc::clone(&pb);
        tasks.spawn(async move {
            let client = PubsubClient::new(&endpoint).await?;
            let (mut stream, _unsubscribe) = client.program_subscribe(&pubkey, None).await?;
            pb.println(format!("subscribe on program updates: {pubkey}"));
            while stream.next().await.is_some() {
                pb.inc(1);
            }
            anyhow::bail!("stream finished");
        });
    }

    if args.transactions {
        for pubkey in accounts.into_iter().chain(owners.into_iter()) {
            let endpoint = args.endpoint.clone();
            let pb = Arc::clone(&pb);
            tasks.spawn(async move {
                let client = PubsubClient::new(&endpoint).await?;
                let (mut stream, _ubsubscribe) = client
                    .logs_subscribe(
                        RpcTransactionLogsFilter::Mentions(vec![pubkey.to_string()]),
                        RpcTransactionLogsConfig { commitment: None },
                    )
                    .await?;
                pb.println(format!("subscribe on transactions for: {pubkey}"));
                while stream.next().await.is_some() {
                    pb.inc(1);
                }
                anyhow::bail!("stream finished");
            });
        }
    }

    loop {
        tokio::select! {
            _ = sleep(Duration::from_millis(10)) => {
                pb.tick();
            }
            result = tasks.join_next() => match result {
                Some(result) => result??,
                None => break,
            }
        }
    }

    Ok(())
}
