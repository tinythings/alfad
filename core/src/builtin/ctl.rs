use std::{ops::ControlFlow, path::Path, time::Duration};

use crate::config::{payload::Payload, yaml::TaskConfigYaml};
use anyhow::Result;
use smallvec::smallvec;
use smol::{fs::File, io::{AsyncBufReadExt, BufReader}, lock::RwLock};
use tracing::{info, error};

use crate::{builtin_fn, task::{ContextMap, TaskContext, TaskState}};

use super::IntoConfig;

pub struct CreateCtlPipe;

impl IntoConfig for CreateCtlPipe {
    fn into_config(self) -> TaskConfigYaml {
        TaskConfigYaml {
            name: "builtin:create-ctl".to_string(),
            cmd: Payload::Normal("mkdir -p /run/var\nmkfifo /run/var/alfad-ctl".to_string()),
            after: smallvec!["mount-sys-fs".to_owned()],
            ..Default::default()
        }
    }
    
}


builtin_fn!(WaitForCommands: wait_for_commands);

impl IntoConfig for WaitForCommands {
    fn into_config(self) -> TaskConfigYaml {
        TaskConfigYaml {
            name: "builtin:wait-for-command".to_string(),
            after: smallvec!["builtin:create-ctl".to_owned()],
            cmd: Self::box_fn(),
            ..Default::default()
        }
    }
}


async fn wait_for_commands(_: &RwLock<TaskContext>, context: ContextMap<'static>) -> ControlFlow<TaskState> {
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
