use std::{fmt::Debug, ops::ControlFlow, str::FromStr};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{command_line::CommandLines, task::{ContextMap, ExitReason, TaskContext, TaskState}, builtin::BuiltInService};


#[async_trait::async_trait]
pub trait Runnable {
    async fn run<'a>(&'a self, context: &'a TaskContext, context_map: ContextMap<'static>) -> ControlFlow<TaskState>;
}

#[derive(Serialize, Deserialize)]
pub enum Payload<T = CommandLines> {
    Marker,
    Service(T),
    #[serde(skip)]
    Builtin(BuiltInService),
}

impl Payload {

    pub async fn run(&self, x: usize, context: &TaskContext, context_map: ContextMap<'static>) -> ControlFlow<TaskState> {

        match self {
            Payload::Service(command_lines) => match command_lines.get(x) {
                Some(command_line) => command_line.run(context, context_map).await,
                None => ControlFlow::Break(TaskState::Concluded(ExitReason::Done))
            },
            Payload::Builtin(runnable) if x == 0 => runnable.run(context, context_map).await,
            _ => ControlFlow::Break(TaskState::Concluded(ExitReason::Done)),
        }
    }
    
    pub(crate) fn is_marker(&self) -> bool {
        matches!(self, Self::Marker)
    }
}

impl<T: Debug + DeserializeOwned> Debug for Payload<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Service(arg0) => f.debug_tuple("Service").field(arg0).finish(),
            Self::Builtin(_) => f.write_str("<builtin>"),
            Self::Marker => f.write_str("<marker>"),
        }
    }
}

impl<T: Default + DeserializeOwned> Default for Payload<T> {
    fn default() -> Self {
        Self::Service(T::default())
    }
}

impl<T: FromStr + DeserializeOwned> FromStr for Payload<T> {
    type Err = <T as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::Service(T::from_str(s)?))
    }
}