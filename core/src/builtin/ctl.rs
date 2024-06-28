use super::IntoConfig;
use crate::{
    builtin_fn,
    def::{APLT_CTL, DIR_RUN},
    task::{ContextMap, TaskContext, TaskState},
};
use crate::{config::yaml::TaskConfigYaml, task::ExitReason};
use anyhow::Result;
use nix::{sys::stat::Mode, unistd::mkfifo};
use smallvec::smallvec;
use smol::{
    fs::{create_dir_all, File},
    io::{AsyncBufReadExt, BufReader},
};
use std::{
    ops::ControlFlow,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::{error, info};

builtin_fn!(CreateCtlPipe: create_ctl);

impl IntoConfig for CreateCtlPipe {
    fn into_config(self) -> TaskConfigYaml {
        TaskConfigYaml {
            name: "builtin::ctl::create".to_string(),
            cmd: Self::box_fn(),
            after: smallvec!["feature::fs::run".to_owned()],
            ..Default::default()
        }
    }
}

async fn create_ctl(_: &TaskContext, _context: ContextMap<'static>) -> Result<()> {
    create_dir_all(DIR_RUN).await?;
    mkfifo(&PathBuf::from(DIR_RUN).join(APLT_CTL), Mode::S_IRWXU | Mode::S_IWOTH)?;
    Ok(())
}

builtin_fn!(WaitForCommands: wait_for_commands);

impl IntoConfig for WaitForCommands {
    fn into_config(self) -> TaskConfigYaml {
        TaskConfigYaml {
            name: "builtin::ctl::daemon".to_string(),
            after: smallvec!["builtin::ctl::create".to_owned()],
            cmd: Self::box_fn(),
            ..Default::default()
        }
    }
}

async fn wait_for_commands(context: &TaskContext, context_map: ContextMap<'static>) -> Result<()> {
    let mut buf = String::new();
    loop {
        if context.state().await == TaskState::Terminating {
            context.update_state(TaskState::Concluded(ExitReason::Terminated)).await;
            break Ok(());
        };
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
                    if let Err(error) = crate::perform_action::perform(action, context_map).await {
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
    Ok(BufReader::new(
        smol::fs::OpenOptions::new()
            .read(true)
            .open(&Path::new(if cfg!(debug_assertions) { "test" } else { DIR_RUN }).join(APLT_CTL))
            .await?,
    ))
}
