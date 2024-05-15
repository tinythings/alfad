use std::{fs, path::PathBuf};

use alfad::config::read_yaml_configs;
use anyhow::Result;
use clap::Parser;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;


#[derive(Parser)]
struct Cli {
    target: PathBuf
}

fn main() -> Result<()>{
    let cli = Cli::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let configs = (alfad::VERSION, read_yaml_configs(&cli.target.join("alfad.d")));
    fs::write(cli.target.join("alfad.bin"), postcard::to_allocvec(&configs)?)?;
    Ok(())
}
