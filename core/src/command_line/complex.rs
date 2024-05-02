use std::{
    env, ops::{Deref, DerefMut}, process::ExitStatus, slice::Iter, str::FromStr
};

use lazy_static::lazy_static;
use regex::{Captures, Regex};
use serde::{de::Visitor, Deserialize};
use smol::process::Command;
use thiserror::Error;

#[derive(Debug)]
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
        command.args(args);
        if self.ignore_env {
            command.env_clear();
        }
        Ok(command)
    }

    pub fn spawn(&self) -> Result<Child, CommandLineError> {
        Ok(Child(self.to_command()?.spawn()?, self.ignore_return))
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

#[derive(Debug, Default)]
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

struct CommandLineVisitor;

impl<'de> Deserialize<'de> for CommandLines {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(CommandLineVisitor)
    }
}

impl<'de> Visitor<'de> for CommandLineVisitor {
    type Value = CommandLines;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("multiline string consisting of one valid command per line")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let x = v
            .lines()
            .map(str::trim)
            .map(CommandLine::from_str)
            .collect::<Result<_, CommandLineError>>();
        match x {
            Ok(list) => Ok(CommandLines(list)),
            Err(e) => Err(E::custom(e)),
        }
    }
}

pub struct Child(smol::process::Child, bool);

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
        let r = insert_envvars("$TEST_VAR_MULTIPLE_1 $TEST_VAR_MULTIPLE_2 $TEST_VAR_MULTIPLE_1").unwrap();
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
