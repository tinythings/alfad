use crate::{
    config::payload::Runnable,
    task::{ContextMap, ExitReason, TaskContext, TaskState},
};
use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use smol::process::Command;
use std::{
    env,
    ops::{ControlFlow, Deref, DerefMut},
    process::{ExitStatus, Stdio},
    slice::Iter,
    str::FromStr,
};
use thiserror::Error;
use tracing::{debug, error, info};

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandLine {
    ignore_env: bool,
    ignore_return: bool,
    args: Vec<String>,
}

const MAX_ENVVAR_RECURSION: usize = 100;

lazy_static! {
    static ref FIND_ENVVAR: Regex = Regex::new(r"\$([_a-zA-Z0-9]+)").unwrap();
}

#[derive(Debug, Error)]
pub enum CommandLineError {
    #[error("Invalid Command: {}", .0)]
    InvalidCommand(String),
    #[error("Empty Command")]
    EmptyCommand,
    #[error(
        "Maximum recursion depth of {} was reached during resolution of environment variables",
        MAX_ENVVAR_RECURSION
    )]
    MaximumRecursion,
    #[error(transparent)]
    IO(#[from] smol::io::Error),
}

impl CommandLine {
    pub fn to_args(&self) -> Result<Vec<String>, CommandLineError> {
        self.args.iter().map(|s| insert_envvars(s)).collect()
    }

    pub fn to_command(&self) -> Result<Command, CommandLineError> {
        let mut args = self.to_args()?.into_iter();
        let program = args.next().ok_or(CommandLineError::EmptyCommand)?;
        let mut command = Command::new(program);
        command.stderr(Stdio::inherit()).stdout(Stdio::inherit());
        command.args(args);
        if self.ignore_env {
            command.env_clear();
        }
        Ok(command)
    }

    pub fn spawn(&self) -> Result<Child, CommandLineError> {
        Ok(Child(self.to_command()?.spawn()?, self.ignore_return))
    }

    async fn run_line(&self, context: &TaskContext) -> ControlFlow<TaskState> {
        // let mut context = context.write().await;

        debug!(cmd = ?self.args, "Running");
        let mut child = match self.spawn() {
            Ok(c) => c,
            Err(CommandLineError::EmptyCommand) => return ControlFlow::Continue(()),
            Err(e) => {
                error!(%e);
                return ControlFlow::Break(TaskState::Concluded(ExitReason::Failed));
            }
        };

        (*context.child.write().await) = Some(child.id() as i32);

        match child.status().await {
            Ok(status) if status.success() => {
                info!(?status);
                (*context.child.write().await) = None;
                ControlFlow::Continue(())
            }
            status => {
                error!(exit = ?status);
                ControlFlow::Break(TaskState::Concluded(ExitReason::Failed))
            }
        }
    }
}

#[async_trait::async_trait]
impl Runnable for CommandLine {
    async fn run<'a>(
        &'a self,
        context: &'a TaskContext,
        _context_map: ContextMap<'static>,
    ) -> ControlFlow<TaskState> {
        self.run_line(context).await
    }
}

impl FromStr for CommandLine {
    type Err = CommandLineError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (s, ignore_env) = prefix_to_flag(s, ':');
        let (s, ignore_return) = prefix_to_flag(s, '-');
        let args = shlex::split(s).ok_or_else(|| CommandLineError::InvalidCommand(s.to_owned()))?;
        Ok(Self {
            ignore_env,
            ignore_return,
            args,
        })
    }
}

fn prefix_to_flag(s: &str, prefix: char) -> (&str, bool) {
    if let Some(s) = s.strip_prefix(prefix) {
        (s, true)
    } else {
        (s, false)
    }
}

fn insert_envvars(s: &str) -> Result<String, CommandLineError> {
    let mut haystack = s.to_owned();
    for _ in 0..MAX_ENVVAR_RECURSION {
        let new = FIND_ENVVAR
            .replace_all(&haystack, |caps: &Captures| {
                env::var(caps.get(1).unwrap().as_str()).unwrap_or_default()
            })
            .to_string();
        if new == haystack {
            return Ok(new);
        }
        haystack = new;
    }
    Err(CommandLineError::MaximumRecursion)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CommandLines(Vec<CommandLine>);

impl<'a> IntoIterator for &'a CommandLines {
    type Item = &'a CommandLine;

    type IntoIter = Iter<'a, CommandLine>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Deref for CommandLines {
    type Target = Vec<CommandLine>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CommandLines {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromStr for CommandLines {
    type Err = CommandLineError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(CommandLines(
            s.lines()
                .map(CommandLine::from_str)
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }
}

#[derive(Debug)]
pub struct Child(pub smol::process::Child, pub bool);

impl Child {
    pub async fn status(&mut self) -> Result<ExitStatus, std::io::Error> {
        let exit = self.0.status().await;
        if self.1 {
            return Ok(ExitStatus::default());
        }
        exit
    }

    pub(crate) fn id(&self) -> u32 {
        self.0.id()
    }
}

#[cfg(test)]
mod test {
    use std::env;

    use super::insert_envvars;
    // WARNING: All ENVVARS must have unique names since the test might run
    // in parallel inside one process which could cause race conditions

    #[test]
    fn replace_simple_var() {
        env::set_var("TEST_VAR_SIMPLE", "foo");
        let r = insert_envvars("$TEST_VAR_SIMPLE").unwrap();
        assert_eq!(r, "foo");
    }

    #[test]
    fn replace_var_in_text() {
        env::set_var("TEST_VAR_IN_TEXT", "foo");
        let r = insert_envvars("Hello my beautiful $TEST_VAR_IN_TEXT, i love you all").unwrap();
        assert_eq!(r, "Hello my beautiful foo, i love you all");
    }

    #[test]
    fn replace_multiple() {
        env::set_var("TEST_VAR_MULTIPLE_1", "foo");
        env::set_var("TEST_VAR_MULTIPLE_2", "bar");
        let r = insert_envvars("$TEST_VAR_MULTIPLE_1 $TEST_VAR_MULTIPLE_2 $TEST_VAR_MULTIPLE_1")
            .unwrap();
        assert_eq!(r, "foo bar foo");
    }

    #[test]
    fn replace_unset_with_empty() {
        let r = insert_envvars("$TEST_VAR_DOES_NOT_EXIST").unwrap();
        assert_eq!(r, "");
    }

    #[test]
    fn catch_infinite_recursion() {
        env::set_var("TEST_VAR_INF_REC_1", "$TEST_VAR_2");
        env::set_var("TEST_VAR_2", "$TEST_VAR_INF_REC_1");
        insert_envvars("$TEST_VAR_INF_REC_1").unwrap_err();
    }
}
