use std::{str::FromStr, sync::Arc};

use crate::{action::{Action, ActionError}, task::{ContextMap, TaskContext, TaskState}};
use nix::sys::signal::Signal;
use smol::lock::RwLock;

pub async fn perform<'a>(s: &'a str, context: &ContextMap<'_>) -> Result<(), ActionError> {
    match Action::from_str(s)? {
        Action::Kill { task, force } => {
            kill(task, force, context).await?;
        }
        Action::Restart { task, force } => {
            kill(task.clone(), force, context).await?;
            start(task, force, context).await?;
        }
        Action::Start { task, force } => start(task, force, context).await?
    }
    Ok(())
}

async fn kill(task: String, force: bool, context: &ContextMap<'_>) -> Result<(), ActionError> {
    let context = get_context(context, &task)?;
    let mut context = context.write().await;
    if force {
        context.send_signal(Signal::SIGKILL);
        context.update_state(crate::task::TaskState::Terminated);

    } else {
        context.send_signal(Signal::SIGTERM);
        context.update_state(crate::task::TaskState::Terminating);
    }
    Ok(())
}

async fn start(task: String, force: bool, context: &ContextMap<'_>) -> Result<(), ActionError> {
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
    context: &ContextMap<'a>,
    name: &str,
) -> Result<&'a Arc<RwLock<TaskContext>>, ActionError> {
    if let Some(context) = context.get(name) {
        Ok(context)
    } else {
        Err(ActionError::TaskNotFound(name.to_owned()))
    }
}
