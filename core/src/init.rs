use crate::config::read_config;
use crate::config::yaml::TaskConfigYaml;

use anyhow::Result;
use futures::StreamExt;
use nix::{
    libc::{SIGABRT, SIGCHLD, SIGHUP, SIGPIPE, SIGTERM, SIGTSTP},
    sys::wait::waitpid,
    unistd::Pid,
};
use signal_hook::{iterator::exfiltrator::WithOrigin, low_level::siginfo::Origin};
use signal_hook_async_std::SignalsInfo;
use std::env;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::task::{ContextMap, Task};

const SIGS: &[i32] = &[SIGABRT, SIGTERM, SIGCHLD, SIGHUP, SIGPIPE, SIGTSTP];

pub struct Alfad {
    pub builtin: Vec<TaskConfigYaml>,
}

impl Alfad {
    pub fn run(self) -> Result<()>{
        let mut signals = SignalsInfo::<WithOrigin>::new(SIGS).unwrap();

        smol::spawn(async move {
            while let Some(sig) = signals.next().await {
                match sig {
                    Origin {
                        signal: SIGCHLD,
                        process: Some(proc),
                        ..
                    } => {
                        // Ignore Err(_) since ECHILD is expected
                        waitpid(Some(Pid::from_raw(proc.pid)), None).ok();
                    }
                    _ => {}
                }
            }
        })
        .detach();

        env::set_var("SMOL_THREADS", "8");
        let subscriber = FmtSubscriber::builder()
            .with_max_level(Level::WARN)
            .finish();

        tracing::subscriber::set_global_default(subscriber)
            .expect("setting default subscriber failed");
        info!("Starting alfad");
        let configs = Box::leak(Box::new(read_config(self.builtin)));
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
        // smol::block_on(async { wait_for_commands(context).await });
        smol::block_on(smol::Timer::never());
        Ok(())
    }
}
