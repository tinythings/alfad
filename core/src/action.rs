use std::str::FromStr;

use clap::Parser;
use thiserror::Error;

#[derive(Debug, Parser)]
pub enum Action {
    /// Kill a task
    Kill {
        task: String,
        #[clap(long)]
        /// Send SIGKILL instead of SIGTERM
        force: bool,
    },
    /// Kill a task and prevent respawn
    Deactivate {
        task: String,
        #[clap(long)]
        /// Send SIGKILL instead of SIGTERM
        force: bool,
    },
    /// Start a task
    Start {
        task: String,
        #[clap(long)]
        /// Ignore conditions and start immediately
        force: bool,
    },
    /// Restart a task
    Restart {
        task: String,
        #[clap(long)]
        /// Ignore conditions and restart immediately
        force: bool,
    },
}

impl FromStr for Action {
    type Err = ActionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((action, payload)) = s.split_once(' ') {
            let task = payload.to_owned();
            match action {
                "kill" => Ok(Action::Kill { task, force: false }),
                "force-kill" => Ok(Action::Kill { task, force: true }),
                "deactivate" => Ok(Action::Kill { task, force: false }),
                "force-deactivate" => Ok(Action::Kill { task, force: true }),
                "restart" => Ok(Action::Restart { task, force: false }),
                "force-restart" => Ok(Action::Restart { task, force: true }),
                "start" => Ok(Action::Start { task, force: false }),
                "force-start" => Ok(Action::Start { task, force: true }),
                _ => Err(ActionError::ActionNotFound(s.to_owned())),
            }
        } else {
            Err(ActionError::SyntaxError(s.to_owned()))
        }
    }
}

impl ToString for Action {
    fn to_string(&self) -> String {
        match self {
            Action::Kill { task, force: false } => format!("kill {task}"),
            Action::Kill { task, force: true } => format!("force-kill {task}"),
            Action::Deactivate { task, force: false } => format!("deactivate {task}"),
            Action::Deactivate { task, force: true } => format!("force-deactivate {task}"),
            Action::Start { task, force: false } => format!("start {task}"),
            Action::Start { task, force: true } => format!("force-start {task}"),
            Action::Restart { task, force: false } => format!("restart {task}"),
            Action::Restart { task, force: true } => format!("force-restart {task}"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("Could not parse command '{}'", .0)]
    SyntaxError(String),
    #[error("Unknown action '{}'", .0)]
    ActionNotFound(String),
    #[error("Task does not exist '{}'", .0)]
    TaskNotFound(String),
}
