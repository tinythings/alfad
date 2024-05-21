use std::str::FromStr;

use clap::{Parser, ValueEnum};
use thiserror::Error;
use strum::{Display, EnumIter};

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
    System {
        command: SystemCommand,
    },
}

#[derive(Parser, Debug, Clone, ValueEnum, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum SystemCommand {
    Poweroff,
    Restart,
    Halt
}

impl FromStr for Action {
    type Err = ActionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let c = if let Some((action, payload)) = s.split_once(' ') {
            let task = payload.to_owned();
            match action {
                "kill" => Action::Kill { task, force: false },
                "force-kill" => Action::Kill { task, force: true },
                "deactivate" => Action::Kill { task, force: false },
                "force-deactivate" => Action::Kill { task, force: true },
                "restart" => Action::Restart { task, force: false },
                "force-restart" => Action::Restart { task, force: true },
                "start" => Action::Start { task, force: false },
                "force-start" => Action::Start { task, force: true },
                "system" => Action::System {
                    command: match payload {
                        "poweroff" => SystemCommand::Poweroff,
                        "restart" => SystemCommand::Restart,
                        "halt" => SystemCommand::Halt,
                        _ => return Err(ActionError::ActionNotFound(s.to_owned())),
                    },
                },
                _ => return Err(ActionError::ActionNotFound(s.to_owned())),
            }
        } else {
            return Err(ActionError::SyntaxError(s.to_owned()));
        };
        Ok(c)
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
            Action::System { command } => format!("system {command}"),
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
