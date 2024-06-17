use std::{ops::ControlFlow, path::Path, time::Duration};

use crate::config::yaml::TaskConfigYaml;
use anyhow::Result;
use nix::{sys::stat::Mode, unistd::mkfifo};
use smallvec::smallvec;
use smol::{fs::{create_dir_all, File}, io::{AsyncBufReadExt, BufReader}, lock::RwLock};
use tracing::{info, error};

use crate::{builtin_fn, task::{ContextMap, TaskContext, TaskState}};

use super::IntoConfig;

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

async fn create_ctl(_: &RwLock<TaskContext>, _context: ContextMap<'static>) -> Result<()> {
    create_dir_all("/run/var").await?;
    mkfifo("/run/var/alfad-ctl", Mode::S_IRWXU | Mode::S_IWOTH)?;
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


async fn wait_for_commands(_: &RwLock<TaskContext>, context: ContextMap<'static>) -> Result<()> {
    let mut buf = String::new();
    smol::block_on(async {
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
                        if let Err(error) = crate::perform_action::perform(action, context).await {
                            error!(%error);
                        }
                    }
                    _ => break,
                }

                buf.clear();
            }
        }
    })
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
