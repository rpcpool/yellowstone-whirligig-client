use tracing_subscriber::{
    filter::{EnvFilter, LevelFilter},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

pub fn tracer_init() -> anyhow::Result<()> {
    let is_atty = atty::is(atty::Stream::Stdout) && atty::is(atty::Stream::Stderr);
    let io_layer = tracing_subscriber::fmt::layer().with_ansi(is_atty);

    let env_layer = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(io_layer)
        .with(env_layer)
        .try_init()?;

    Ok(())
}
