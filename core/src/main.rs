pub mod action;
pub mod config;
pub mod ordering;
mod perform_action;
pub mod task;
mod validate;

use anyhow::Result;
use config::read_config;
use nix::{sys::stat, unistd::mkfifo};
use std::{fs::remove_file, time::Duration};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use smol::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};
use task::{ContextMap, Task};

#[allow(dead_code)]
static VERSION: &str = "0.1";

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let configs = Box::leak(Box::new(read_config()));
    let context: ContextMap = Box::leak(Box::new(
        configs
            .iter()
            .map(|config| (config.name.as_str(), Default::default()))
            .collect(),
    ));

    configs
        .iter()
        .for_each(|config| Task::spawn(config, context));
    smol::block_on(async { wait_for_commands(context).await });
}

async fn wait_for_commands(context: ContextMap<'static>) {
    let mut buf = String::new();
    loop {
        let mut pipe = match create_pipe().await {
            Ok(x) => x,
            Err(error) => {
                error!("{error}");
                smol::Timer::after(Duration::from_secs(10));
                continue;
            }
        };
        loop {
            match pipe.read_line(&mut buf).await {
                Ok(bytes) if bytes > 0 => {
                    let action = buf.trim();
                    info!(action);
                    if let Err(error) = perform_action::perform(action, &context).await {
                        error!(%error);
                    }
                }
                _ => { break }
            }

            buf.clear();
        }
    }
}

async fn create_pipe() -> Result<BufReader<File>> {
    // let path = "/var/run/alfad";
    let path = "test/alfad-pipe";
    remove_file(path)?;
    mkfifo(path, stat::Mode::S_IRWXU)?;
    let file = smol::fs::OpenOptions::new().read(true).open(path).await?;
    Ok(BufReader::new(file))
}
