use std::io::IsTerminal;
use anyhow::Result;
use fabric_rs::{App, app::ARGS};

use tracing_subscriber::{
    prelude::*,
    filter::EnvFilter,
};

fn main() -> Result<()> {
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr);

    let tracer = tracing_subscriber::registry()
        .with(EnvFilter::from_default_env());

    if std::io::stderr().is_terminal() {
        tracer
            .with(fmt_layer)
            .init();
    } else {
        tracer
            .with(fmt_layer.json())
            .init();
    }

    let app = App::default();
    app.run(&ARGS)
}
