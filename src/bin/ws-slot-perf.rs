use {
    clap::Parser,
    futures::{sink::SinkExt, stream::StreamExt},
    maplit::hashmap,
    solana_client::{nonblocking::pubsub_client::PubsubClient, rpc_response::SlotInfo},
    std::{collections::BTreeMap, time::Instant},
    yellowstone_grpc_client::GeyserGrpcClient,
    yellowstone_grpc_proto::prelude::{
        subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
        SubscribeRequestFilterSlots, SubscribeUpdate, SubscribeUpdateSlot,
    },
};

#[derive(Debug, Clone, Parser)]
struct Args {
    /// Solana PubSubWebSocket endpoint
    #[clap(long)]
    pubsub: String,

    /// Triton Whirligig WebSocket endpoint
    #[clap(long)]
    whirligig: String,

    /// Triton gRPC endpoint
    #[clap(long)]
    grpc: String,

    /// Triton gRPC endpoint x-token
    #[clap(long)]
    x_token: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Slots {
    pubsub: Option<Instant>,
    whirligig: Option<Instant>,
    grpc: Option<Instant>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let client_pubsub = PubsubClient::new(&args.pubsub).await?;
    let mut subscribe_pubsub = client_pubsub.slot_subscribe().await?.0;

    let client_whirligig = PubsubClient::new(&args.whirligig).await?;
    let mut subscribe_whirligig = client_whirligig.slot_subscribe().await?.0;

    let mut client = GeyserGrpcClient::connect(args.grpc, args.x_token, None)?;
    let (mut subscribe_tx, mut grpc) = client.subscribe().await?;
    subscribe_tx
        .send(SubscribeRequest {
            slots: hashmap! { "".to_owned() => SubscribeRequestFilterSlots { filter_by_commitment: Some(true) } },
            commitment: Some(CommitmentLevel::Processed as i32),
            ..Default::default()
        })
        .await?;

    let mut slots = BTreeMap::<u64, Slots>::new();
    loop {
        let slot = tokio::select! {
            Some(SlotInfo{slot,..}) = subscribe_pubsub.next() => {
                slots.entry(slot).or_default().pubsub = Some(Instant::now());
                slot
            }
            Some(SlotInfo{slot,..}) = subscribe_whirligig.next() => {
                slots.entry(slot).or_default().whirligig = Some(Instant::now());
                slot
            }
            Some(msg) = grpc.next() => {
                if let SubscribeUpdate { update_oneof: Some(UpdateOneof::Slot(SubscribeUpdateSlot { slot, .. })), .. } = msg? {
                    slots.entry(slot).or_default().grpc = Some(Instant::now());
                    slot
                } else {
                    continue
                }
            }
        };

        if let Some(Slots {
            pubsub: Some(pubsub),
            whirligig: Some(whirligig),
            grpc: Some(grpc),
        }) = slots.get(&slot).copied()
        {
            // The issue why PubSub looks better: https://github.com/solana-labs/solana/issues/32958

            let first = pubsub.min(whirligig.min(grpc));
            println!(
                "{slot}: pubsub +{:?}, whirligig +{:?}, grpc +{:?}",
                pubsub.saturating_duration_since(first),
                whirligig.saturating_duration_since(first),
                grpc.saturating_duration_since(first)
            );

            loop {
                match slots.keys().next().copied() {
                    Some(first_slot) if first_slot <= slot => {
                        slots.remove(&first_slot);
                    }
                    _ => break,
                }
            }
        }
    }
}
