use std::{
    collections::HashMap,
    future::Future,
    ops::{ControlFlow, Deref},
    pin::{pin, Pin},
    task::{Context, Poll, Waker},
};

use strum::Display;

use nix::{sys::signal::Signal, unistd::Pid};

use tracing::{debug, error, info, trace, trace_span};

use serde::Deserialize;
use smol::{
    lock::{RwLock, RwLockUpgradableReadGuard},
    ready,
};

use crate::config::{payload::Payload, Respawn, TaskConfig};

#[derive(Debug, Clone, Copy)]
pub struct ContextMap<'a>(pub &'a HashMap<&'a str, TaskContext>);

pub struct TaskWaiter<'a, F: Fn(&TaskState) -> bool> {
    context: &'a TaskContext,
    predicate: F,
}

impl<'a, F: Fn(&TaskState) -> bool> Future for TaskWaiter<'a, F> {
    type Output = TaskState;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let _x = trace_span!("TaskWaiter").entered();
        trace!("Checking {}", self.context.config.name);
        let mut state_manager = ready!(pin!(self.context.state_manager.write()).poll(cx));
        trace!("{} is {}", self.context.config.name, state_manager.state);
        if (self.predicate)(&state_manager.state) {
            Poll::Ready(state_manager.state)
        } else {
            let waker = cx.waker().clone();
            trace!(?waker, "Waiting");
            state_manager.wakers.push(waker);
            Poll::Pending
        }
    }
}

impl<'a> ContextMap<'a> {
    pub async fn wait_for(&self, other: &str, state: TaskState) -> Option<TaskState> {
        match self.0.get(other) {
            Some(task) => Some(
                TaskWaiter {
                    context: task,
                    predicate: |x| *x == state,
                }
                .await,
            ),
            None => None,
        }
    }

    pub async fn wait_for_running(&self, other: &str) -> Option<TaskState> {
        match self.0.get(other) {
            Some(task) => Some(
                TaskWaiter {
                    context: task,
                    predicate: TaskState::is_running,
                }
                .await,
            ),
            None => None,
        }
    }

    pub async fn wait_for_conclusion(&self, other: &str) -> Option<TaskState> {
        match self.0.get(other) {
            Some(task) => Some(
                TaskWaiter {
                    context: task,
                    predicate: TaskState::has_concluded,
                }
                .await,
            ),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Display, Hash)]
pub enum TaskState {
    Created,
    Waiting,
    Running(usize),
    Concluded(ExitReason),
    Terminating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Display, Hash)]
pub enum ExitReason {
    Done,
    Failed,
    Terminated,
    Deactivated
}

impl TaskState {
    pub fn has_concluded(&self) -> bool {
        matches!(self, Self::Concluded(_))
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running(_))
    }

    pub fn is_waiting(&self) -> bool {
        *self == Self::Waiting
    }
}

impl Default for TaskState {
    fn default() -> Self {
        Self::Created
    }
}

pub fn spawn(context: &'static TaskContext, context_map: ContextMap<'static>) {
    if matches!(
        context.config.payload,
        Payload::Service(_) | Payload::Builtin(_)
    ) {
        info!("Spawning {}", context.config.name);
    }
    smol::spawn(async move { drive(context, context_map).await }).detach()
}

pub async fn drive(context: &'static TaskContext, context_map: ContextMap<'static>) {
    loop {
        context.update_state(TaskState::Waiting).await;
        for task in context.config.with.iter() {
            trace!("{} waiting for {task} to be Running", context.config.name);
            if context_map.wait_for_running(task).await.is_none() {
                context.update_state(TaskState::Concluded(ExitReason::Deactivated)).await;
                return;
            }
        }

        if context.config.payload.is_marker() {
            context.update_state(TaskState::Running(0)).await;
        }

        for task in context.config.after.iter() {
            trace!("{} waiting for {task} to be Done", context.config.name);
            if context_map
                .wait_for(task, TaskState::Concluded(ExitReason::Done))
                .await.is_none()
            {
                context.update_state(TaskState::Concluded(ExitReason::Deactivated)).await;
                return;
            }
        }

        if context.config.payload.is_marker() {
            context.update_state(TaskState::Concluded(ExitReason::Done)).await;
            break;
        }

        // Running
        let mut index = 0;
        loop {
            debug!(task = context.config.name, cmd = index);
            context.update_state(TaskState::Running(index)).await;
            match context
                .config
                .payload
                .run(index, context, context_map)
                .await
            {
                ControlFlow::Continue(_) => {
                    index += 1;
                }
                ControlFlow::Break(payload_state) => {
                    let current_state = context.state().await;
                    let state = match (current_state, payload_state) {
                        (TaskState::Terminating, _) => TaskState::Concluded(ExitReason::Terminated),
                        (_, state) => state,
                    };
                    context.update_state(state).await;
                    info!(task = context.config.name, %state ,"Breaking");
                    break;
                }
            }
        }

        // Respawn
        match context.config.respawn {
            Respawn::Retry(max_attempts) => {
                let mut attempts = context.respawn_attempts.write().await;
                if *attempts < max_attempts {
                    *attempts += 1;
                } else {
                    break;
                }
            }
            Respawn::No => break,
        }
    }
}

#[derive(Debug, Default)]
pub struct TaskContext {
    pub config: TaskConfig,
    state_manager: RwLock<StateManager>,
    pub child: RwLock<Option<i32>>,
    pub respawn_attempts: RwLock<usize>,
}

#[derive(Debug, Default)]
pub struct StateManager {
    pub state: TaskState,
    pub wakers: Vec<Waker>,
    pub waker: Option<Waker>,
}

impl TaskContext {
    pub fn new(config: TaskConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    pub async fn update_state(&self, state: TaskState) {
        let manager = self.state_manager.upgradable_read().await;
        if manager.state != state {
            let mut manager = RwLockUpgradableReadGuard::upgrade(manager).await;
            manager.state = state;
            manager.wakers.drain(..).for_each(Waker::wake);
        }
    }

    pub async fn state(&self) -> TaskState {
        self.state_manager.read().await.state
    }

    pub async fn send_signal(&self, signal: Signal) {
        if let Some(child) = self.child.read().await.deref() {
            let pid = Pid::from_raw(*child);
            if let Err(error) = nix::sys::signal::kill(pid, signal) {
                error!("{error}");
            }
        } else {
            error!("{} has no running process", self.config.name)
        }
    }

    pub async fn set_waker(&self, waker: &Waker) {
        self.state_manager
            .write()
            .await
            .waker
            .get_or_insert_with(|| waker.clone())
            .clone_from(waker);
    }

    pub async fn wake(&self) {
        let x = &self.state_manager.read().await.waker;
        if let Some(waker) = x {
            waker.wake_by_ref();
        };
    }
}
