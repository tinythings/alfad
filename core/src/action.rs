use crate::def::{APLT_COMPILE, APLT_CTL, APLT_INIT, APLT_MAIN};
use clap::{Parser, ValueEnum};
use std::{
    fmt::{Debug, Display},
    str::FromStr,
};
use strum::{Display, EnumIter};
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
    System {
        command: SystemCommand,
    },
}

#[derive(Parser, Debug, Clone, ValueEnum, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum SystemCommand {
    Poweroff,
    Restart,
    Halt,
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

impl Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Kill { task, force } => {
                if *force {
                    f.write_str("force-")?;
                }
                f.write_str("kill ")?;
                f.write_str(task)
            }
            Action::Deactivate { task, force } => {
                if *force {
                    f.write_str("force-")?;
                }
                f.write_str("deactivate ")?;
                f.write_str(task)
            }
            Action::Start { task, force } => {
                if *force {
                    f.write_str("force-")?;
                }
                f.write_str("start")?;
                f.write_str(task)
            }
            Action::Restart { task, force } => {
                if *force {
                    f.write_str("force-")?;
                }
                f.write_str("restart ")?;
                f.write_str(task)
            }
            Action::System { command } => {
                f.write_str("system ")?;
                Display::fmt(command, f)
            }
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

    #[error(
        "Do not call this binary directly as {:?}! Name or link to an applet expected instead.
The following applets are available:

  - {}
  - {}
  - {}
",
        APLT_MAIN,
        APLT_INIT,
        APLT_CTL,
        APLT_COMPILE
    )]
    MainAppletCalled,
}
