use std::{fmt::Debug, ops::ControlFlow, str::FromStr};

use serde::{Deserialize, Serialize};
use smol::lock::RwLock;

use crate::{command_line::CommandLines, task::{ContextMap, TaskContext, TaskState}};


#[async_trait::async_trait]
pub trait Runnable {
    async fn run<'a>(&'a self, context: &'a RwLock<TaskContext>, context_map: ContextMap<'static>) -> ControlFlow<TaskState>;
}

pub enum Payload<T = CommandLines> {
    Normal(T),
    Builtin(&'static mut (dyn Runnable + Sync))
}

impl Payload {

    pub async fn run(&self, x: usize, context: &RwLock<TaskContext>, context_map: ContextMap<'static>) -> ControlFlow<TaskState> {

        match self {
            Payload::Normal(command_lines) => match command_lines.get(x) {
                Some(command_line) => command_line.run(context, context_map).await,
                None => ControlFlow::Break(TaskState::Done)
            },
            Payload::Builtin(runnable) if x == 0 => runnable.run(context, context_map).await,
            Payload::Builtin(_) => ControlFlow::Break(TaskState::Done),
        }
    }
}

impl Serialize for Payload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        match self {
            Payload::Normal(n) => n.serialize(serializer),
            Payload::Builtin(_) => Err(<S::Error as serde::ser::Error>::custom("Cannot serialize builtin tasks")),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Payload<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        Ok(Self::Normal(T::deserialize(deserializer)?))
    }
}

impl<T: Debug> Debug for Payload<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal(arg0) => f.debug_tuple("Normal").field(arg0).finish(),
            Self::Builtin(_) => f.write_str("<builtin>"),
        }
    }
}

impl<T: Default> Default for Payload<T> {
    fn default() -> Self {
        Self::Normal(T::default())
    }
}

impl<T: FromStr> FromStr for Payload<T> {
    type Err = <T as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::Normal(T::from_str(s)?))
    }
}