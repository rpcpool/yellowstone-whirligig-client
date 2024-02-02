# Whirligig Client

Client for Triton's Whirligig, Solana WebSocket interface replacement. Based on Dragon's Mouth gRPC.

https://docs.solana.com/api/websocket

## Run client

```
$ cargo run --bin ws-client -- --help
    Finished dev [unoptimized + debuginfo] target(s) in 0.24s
     Running `target/debug/ws-client --help`
Usage: ws-client [OPTIONS] <COMMAND>

Commands:
  subscribe  
  help       Print this message or the help of the given subcommand(s)

Options:
  -e, --endpoint <ENDPOINT>  WebSocket endpoint [default: ws://127.0.0.1:8000/]
  -h, --help                 Print help

$ cargo run --bin ws-client -- subscribe --help
Usage: ws-client subscribe [OPTIONS] <COMMAND>

Commands:
  account                 Subscribe on account updates
  logs                    Subscribe on transactions log updates
  program                 Subscribe on accounts updates owned by program
  signature               Subscribe on transaction confirmation events
  slot                    Subscribe on slot updates
  block                   Subscribe on block updates
  transaction             
  transaction-deprecated  
  help                    Print this message or the help of the given subcommand(s)

Options:
  -c, --commitment <COMMITMENT>  Commitment level of subscritpion [default: finalized] [possible values: processed, confirmed, finalized]
      --only-counter             Show only progress bar with received messages
  -h, --help                     Print help
```

## Run stress test

```
$ cargo run --release --bin ws-stress-test -- --help
Usage: ws-stress-test [OPTIONS]

Options:
  -e, --endpoint <ENDPOINT>  WebSocket endpoint [default: ws://127.0.0.1:8000/streams]
  -a, --accounts <ACCOUNTS>  Path to file with accounts Pubkeys
  -o, --owners <OWNERS>      Path to file with owners Pubkeys, by default some most used programs are used
  -t, --transactions         Additionally subscribe on transactions with both `accounts` and `owners`
  -h, --help                 Print help
```

By default some default accounts would be used, but you can test with specified ones. Accounts would subscribe with `account_subscribe`, owners with `program_subscribe`, accounts and owners both would subscribe with `logs_subscribe`.
