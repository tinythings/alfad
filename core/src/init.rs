use crate::{config::read_config, task::TaskContext};
use crate::config::yaml::TaskConfigYaml;

use anyhow::Result;
use futures::StreamExt;
use nix::
    libc::{SIGABRT, SIGHUP, SIGPIPE, SIGTERM, SIGTSTP}
;
use signal_hook::iterator::exfiltrator::WithOrigin;
use signal_hook_async_std::SignalsInfo;
use std::env;
use tracing::info;

use crate::task::ContextMap;

const SIGS: &[i32] = &[SIGABRT, SIGTERM, SIGHUP, SIGPIPE, SIGTSTP];

pub struct Alfad {
    pub builtin: Vec<TaskConfigYaml>,
}

impl Alfad {
    pub fn run(self) -> Result<()> {
        let mut signals = SignalsInfo::<WithOrigin>::new(SIGS).unwrap();

        smol::spawn(async move {
            loop {
                signals.next().await;
            }
        })
        .detach();

        env::set_var("SMOL_THREADS", "8");
        info!("Starting alfad");
        let configs = read_config(self.builtin);
        let context: ContextMap = ContextMap(Box::leak(Box::new(
            configs
                .into_iter()
                .map(|config| (&*config.name.clone().leak(), TaskContext::new(config)))
                .collect(),
        )));
        info!("Done parsing ({} tasks)", context.0.len());
        context.0
            .values()
            .for_each(|config| crate::task::spawn(config, context));
        // smol::block_on(async { wait_for_commands(context).await });
        smol::block_on(smol::Timer::never());
        Ok(())
    }
}
