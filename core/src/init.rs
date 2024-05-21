use anyhow::Result;
use crate::config::read_config;
use futures::StreamExt;
use nix::{
    libc::{SIGABRT, SIGCHLD, SIGHUP, SIGPIPE, SIGTERM, SIGTSTP}, sys::wait::waitpid, unistd::Pid
};
use signal_hook::{iterator::exfiltrator::WithOrigin, low_level::siginfo::Origin};
use signal_hook_async_std::SignalsInfo;
use std::{env, path::Path, time::Duration};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

use smol::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
};
use crate::task::{ContextMap, Task};

const SIGS: &[i32] = &[SIGABRT, SIGTERM, SIGCHLD, SIGHUP, SIGPIPE, SIGTSTP];

pub fn main() {
    let mut signals = SignalsInfo::<WithOrigin>::new(SIGS).unwrap();

    smol::spawn(async move {
        while let Some(sig) = signals.next().await {
            match sig {
                Origin { signal: SIGCHLD, process: Some(proc), .. } => {
                    // Ignore Err(_) since ECHILD is expected
                    waitpid(Some(Pid::from_raw(proc.pid)), None).ok();
                },
                _ => {}
            }
        }
    })
    .detach();

    env::set_var("SMOL_THREADS", "8");
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    info!("Starting alfad");
    let configs = Box::leak(Box::new(read_config()));
    let context: ContextMap = Box::leak(Box::new(
        configs
            .iter()
            .map(|config| (config.name.as_str(), Default::default()))
            .collect(),
    ));
    info!("Done parsing");
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
                error!("Could not create pipe: {error}");
                smol::Timer::after(Duration::from_secs(10)).await;
                continue;
            }
        };
        loop {
            match pipe.read_line(&mut buf).await {
                Ok(bytes) if bytes > 0 => {
                    let action = buf.trim();
                    info!(action);
                    if let Err(error) = crate::perform_action::perform(action, &context).await {
                        error!(%error);
                    }
                }
                _ => break,
            }

            buf.clear();
        }
    }
}

async fn create_pipe() -> Result<BufReader<File>> {
    let dir = if cfg!(debug_assertions) {
        "test"
    } else {
        "/run/var"
    };
    let path = Path::new(dir).join("alfad-ctl");
    let file = smol::fs::OpenOptions::new().read(true).open(&path).await?;
    Ok(BufReader::new(file))
}
