use std::{ffi::c_int, str::FromStr, time::Duration};

use crate::{
    action::{Action, ActionError, SystemCommand},
    task::{ContextMap, TaskContext, TaskState},
};
use futures::{future::join_all, select, FutureExt};
use nix::{
    libc::{
        c_long, syscall, LINUX_REBOOT_CMD_HALT, LINUX_REBOOT_CMD_POWER_OFF,
        LINUX_REBOOT_CMD_RESTART,
    },
    sys::signal::Signal,
};
use smol::lock::RwLock;
use thiserror::Error;
use tracing::{error, info};

pub async fn perform<'a>(s: &'a str, context: ContextMap<'static>) -> Result<(), ActionError> {
    match Action::from_str(s)? {
        Action::Kill { task, force } => kill_by_name(&task, force, context).await?,
        Action::Deactivate { task, force } => {
            kill_by_name(&task, force, context).await?;
            get_context(context, &task)?
                .write()
                .await
                .update_state(TaskState::Deactivated);
        }
        Action::Restart { task, force } => {
            kill_by_name(&task, force, context).await?;
            start(task, force, context).await?;
        }
        Action::Start { task, force } => start(task, force, context).await?,
        Action::System { command } => match command {
            SystemCommand::Poweroff => {
                info!("Powering off...");
                kill_all(false, context).await;
                let error = fee1dead(LINUX_REBOOT_CMD_POWER_OFF);
                error!("Error {error}");
            }
            SystemCommand::Restart => {
                info!("Restarting...");
                let error = fee1dead(LINUX_REBOOT_CMD_RESTART);
                error!("Error {error}");
            }
            SystemCommand::Halt => {
                info!("Halting...");
                let error = fee1dead(LINUX_REBOOT_CMD_HALT);
                error!("Error {error}");
            }
        },
    }
    Ok(())
}

#[derive(Error, Debug)]
#[error("{}", .0)]
pub struct FailedToKill(&'static str);

async fn kill_all(force: bool, context_map: ContextMap<'static>) -> Vec<Result<(), FailedToKill>> {
    join_all(context_map.iter().map(|(name, context)| async {
        select! {
            _ = async {
                let mut guard = context.write().await;
                kill(&mut guard, force).await;
                guard.wait_for_terminate().await;
            }.fuse() => Ok(()),
            _ = smol::Timer::after(Duration::from_millis(1000)).fuse() => Err(FailedToKill(name))
        }
    }))
    .await
}

fn fee1dead(code: c_int) -> c_long {
    unsafe { syscall(169, 0xfee1deadu32, 537993216, c_long::from(code)) }
}

async fn kill_by_name(task: &str, force: bool, context: ContextMap<'_>) -> Result<(), ActionError> {
    kill(&mut (*get_context(context, task)?.write().await), force).await;
    Ok(())
}

async fn kill(task: &mut TaskContext, force: bool) {
    if task.state().has_concluded() || task.state().is_waiting() {
        return;
    }
    if force {
        task.send_signal(Signal::SIGKILL);
        task.update_state(TaskState::Terminated);
    } else {
        task.send_signal(Signal::SIGTERM);
        task.update_state(TaskState::Terminating);
    }
}

async fn start(task: String, force: bool, context: ContextMap<'_>) -> Result<(), ActionError> {
    let context = get_context(context, &task)?;
    let mut context = context.write().await;
    let new_state = if force {
        TaskState::Starting
    } else {
        TaskState::Waiting
    };
    context.update_state(new_state);
    context.wake();
    Ok(())
}

fn get_context<'a>(
    context: ContextMap<'a>,
    name: &str,
) -> Result<&'a RwLock<TaskContext>, ActionError> {
    if let Some(context) = context.get(name) {
        Ok(context)
    } else {
        Err(ActionError::TaskNotFound(name.to_owned()))
    }
}
