mod actions;
mod task;
mod validate;

use futures::FutureExt;
use nix::{
    libc::remove,
    sys::stat::{self, Mode},
    unistd::mkfifo,
};
use std::{
    collections::HashMap,
    fs::{read_dir, remove_file, OpenOptions},
    sync::Arc,
    time::Duration,
};
use tracing::{error, info, info_span, Level};
use tracing_subscriber::FmtSubscriber;

use smol::{
    fs::File,
    future,
    io::{AsyncBufReadExt, BufReader},
    lock::RwLock,
};
use task::{Task, TaskConfig, TaskContext};

pub type StatusMap<'a> = &'static HashMap<&'a str, Arc<RwLock<TaskContext>>>;

#[allow(dead_code)]
static VERSION: &str = "0.1";

fn main() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let configs = Box::leak(Box::new(read_config()));
    let context: StatusMap = Box::leak(Box::new(
        configs
            .iter()
            .map(|config| (config.name.as_str(), Default::default()))
            .collect(),
    ));

    smol::block_on(async {
        for config in configs.iter() {
            smol::spawn(async move {
                Task::new(config, context).await;
            })
            .detach();
        }
        let mut pipe = create_pipe().await;
        let mut buf = String::new();
        loop {
            if pipe.read_line(&mut buf).await.unwrap() == 0 {
                smol::Timer::after(Duration::from_millis(50)).await;
                continue;
            };
            let action = buf.trim();
            info!(action);
            actions::perform(action, &context).await;
            buf.clear();
        }
    });
}

fn read_config() -> Vec<TaskConfig> {
    let span = info_span!("Parsing task files");
    let _span = span.enter();
    let dir = if cfg!(profile = "release") {
        "/etc/slimit/slimit.d"
    } else {
        "test/slimit.d"
    };
    let configs = read_dir(dir)
        .unwrap()
        .inspect(|path| info!(file = ?path))
        .flatten()
        .map(|file| {
            serde_yaml::from_reader(OpenOptions::new().read(true).open(file.path()).unwrap())
        })
        .inspect(|config| match config {
            Ok(config) => info!(?config),
            Err(error) => error!(%error),
        })
        .flatten()
        .collect();

    #[cfg(feature = "validate")]
    let configs = validate::validate(configs);

    drop(_span);
    configs
}

async fn create_pipe() -> BufReader<File> {
    // let path = "/var/run/slimit";
    let path = "test/slimit-pipe";
    remove_file(path).ok();
    mkfifo(path, stat::Mode::S_IRWXU).unwrap();
    let file = smol::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .await
        .unwrap();
    BufReader::new(file)
}
