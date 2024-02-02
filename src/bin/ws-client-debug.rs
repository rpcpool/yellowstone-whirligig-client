use {
    futures::{future, pin_mut, StreamExt},
    tokio_tungstenite::{connect_async, tungstenite::protocol::Message},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (stdin_tx, stdin_rx) = futures::channel::mpsc::unbounded();
    let text = r#"{"id":1,"jsonrpc":"2.0","method":"slotSubscribe","params":[]}"#.to_owned();
    stdin_tx.unbounded_send(Message::text(text))?;
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(100)).await;
    });

    let (ws_stream, _) = connect_async("ws://127.0.0.1:8000/streams").await?;

    let (write, read) = ws_stream.split();

    let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async move {
            println!("{:?}", message.unwrap());
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;

    Ok(())
}
