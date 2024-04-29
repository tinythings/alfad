use std::{
    collections::HashMap,
    future::Future,
    num::NonZeroU32,
    path::Path,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use enum_display_derive::Display;
use nix::{sys::signal::Signal, unistd::Pid};
use smallvec::SmallVec;
use std::fmt::Display;
use tracing::{error, info, warn};

use serde::Deserialize;
use smol::{
    lock::RwLock,
    process::{Child, Command},
    ready,
};

use crate::config::OneOrMany;

pub type ContextMap<'a> = &'static HashMap<&'a str, Arc<RwLock<TaskContext>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Display)]
pub enum TaskState {
    Waiting,
    Starting,
    Running,
    Done,
    Failed,
    Terminating,
    Terminated,
    Killed,
}

impl Default for TaskState {
    fn default() -> Self {
        Self::Waiting
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub enum Respawn {
    /// Never retry this task (default)
    No,
    /// Restart this task up to N times
    ///
    /// N = 0, restart this task an unlimited number of times
    // TODO: Does manual restart affect the counter, if so: how
    Retry { amount: usize },
    /// Restart this task up to N times but wait between retries
    ///
    /// N = 0, restart this task an unlimited number of times
    // TODO: Does manual restart affect the counter, if so: how
    Timeout { amount: usize, time: Duration },
}

impl Default for Respawn {
    fn default() -> Self {
        Self::No
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct TaskConfig {
    pub name: String,
    #[serde(default)]
    cmd: Vec<String>,
    #[cfg(feature = "before")]
    #[serde(default, deserialize_with = "OneOrMany::read")]
    pub before: Vec<String>,
    #[serde(default, deserialize_with = "OneOrMany::read")]
    pub with: Vec<String>,
    #[serde(default, deserialize_with = "OneOrMany::read")]
    pub after: SmallVec<[String; 1]>,
    #[serde(default)]
    respawn: Respawn,
    pub group: Option<String>,
}

impl TaskConfig {
    pub fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    pub fn after(&mut self, name: &str) -> &mut Self {
        self.after.push(name.to_owned());
        self
    }
}

pub struct Task<'a> {
    pub state: TaskState,
    pub config: &'a TaskConfig,
    pub context_map: &'a HashMap<&'a str, Arc<RwLock<TaskContext>>>,
    context: Arc<RwLock<TaskContext>>,
    pub process: Option<Child>,
}

impl Future for Task<'_> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let p = self.poll_internal(cx);
        ready!(pin!(self.propagate_state()).poll(cx));
        p
    }
}

impl<'a> Task<'a> {
    pub fn trace(&self) {
        info!("{} is {:?}", self.config.name, &self.state);
    }

    pub fn spawn(config: &'static TaskConfig, context_map: ContextMap<'static>) {
        smol::spawn(async move { Self::new(config, context_map).await }).detach()
    }

    pub fn new(
        config: &'a TaskConfig,
        context_map: &'a HashMap<&'a str, Arc<RwLock<TaskContext>>>,
    ) -> Self {
        Self {
            state: TaskState::Waiting,
            config,
            context_map,
            context: context_map
                .get(config.name.as_str())
                .expect("generated from the same list and must thus be in the context_map")
                .clone(),
            process: None,
        }
    }

    fn poll_internal(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let mut context = ready!(pin!(self.context.write()).poll(cx));
        let state = context.state;
        if let Some(waker) = context.waker.as_mut() {
            waker.clone_from(cx.waker());
        } else {
            context.waker = Some(cx.waker().clone());
        }
        // explicitly drop context so we can mutate self again
        drop(context);

        self.state = state;
        loop {
            match self.state {
                TaskState::Waiting => {
                    ready!(self.wait_for_dependencies(cx));
                    self.state = TaskState::Starting
                }
                TaskState::Starting => {
                    ready!(pin!(self.perform()).poll(cx));
                    self.state = TaskState::Running;
                }
                TaskState::Running => {
                    ready!(pin!(self.running()).poll(cx));
                }
                TaskState::Terminating => {
                    ready!(pin!(self.wait_for_terminate()).poll(cx));
                    self.state = TaskState::Terminated
                }
                _ => return Poll::Pending,
            }
        }
    }

    fn wait_for_dependencies(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        ready!(self.wait_for(self.config.after.iter(), TaskState::Done, cx));
        ready!(self.wait_for(self.config.with.iter(), TaskState::Running, cx));
        Poll::Ready(())
    }

    fn wait_for<'b>(
        &mut self,
        list: impl IntoIterator<Item = &'b String>,
        state: TaskState,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
        for name in list {
            if let Some(other) = self.context_map.get(name.as_str()) {
                {
                    let context = ready!(pin!(other.read()).poll(cx));
                    if context.state == state {
                        continue;
                    }
                }
                let mut context = ready!(pin!(other.write()).poll(cx));
                match state {
                    TaskState::Running => context.waiters_running.push(cx.waker().clone()),
                    TaskState::Done => context.waiters_done.push(cx.waker().clone()),
                    _ => {}
                }

                info!("'{}' waiting for '{name}' to be {state}", self.config.name);
            } else {
                warn!(
                    "'{}' is waiting for '{}', which does not exist, and will never run",
                    self.config.name, name
                );
                return Poll::Pending;
            }

            return Poll::Pending;
        }
        Poll::Ready(())
    }

    async fn running(&mut self) {
        if let Some(child) = self.process.as_mut() {
            match child.status().await {
                Ok(status) if status.success() => self.state = TaskState::Done,
                _ if self.state == TaskState::Terminating => self.state = TaskState::Terminated,
                status => {
                    self.state = TaskState::Failed;
                    error!(exit = ?status);
                }
            }
        } else {
            self.state = TaskState::Done;
        }
    }

    async fn wait_for_terminate(&mut self) {
        if let Some(child) = self.process.as_mut() {
            child.status().await.ok();
        }
    }

    async fn perform(&mut self) {
        let mut args = self.config.cmd.iter();
        if let Some(program) = args.next() {
            match Command::new(program).args(args).spawn() {
                Ok(p) => {
                    self.context.write().await.pid = NonZeroU32::new(p.id());
                    self.process = Some(p);
                }
                Err(error) => error!("{error}"),
            }
        }
    }

    async fn propagate_state(&mut self) {
        self.trace();
        let state = self.state;
        self.context.write().await.update_state(state);
    }
}

#[derive(Debug, Default)]
pub struct TaskContext {
    state: TaskState,
    waiters_running: Vec<Waker>,
    waiters_done: Vec<Waker>,
    pid: Option<NonZeroU32>,
    /// used to wake this task from the outside
    waker: Option<Waker>,
}

impl TaskContext {
    pub fn update_state(&mut self, state: TaskState) {
        self.state = state;
        match self.state {
            TaskState::Running => self
                .waiters_running
                .drain(..)
                .for_each(|w| w.wake_by_ref()),
            TaskState::Done => self
                .waiters_done
                .drain(..)
                .for_each(|w| w.wake_by_ref()),
            _ => {}
        }
    }

    pub fn sanity_check(&self) -> bool {
        if let Some(pid) = self.pid {
            Path::new(&format!("/proc/{}", pid)).exists()
        } else {
            false
        }
    }

    pub fn send_signal(&self, signal: Signal) {
        if let Some(pid) = self.pid {
            if !self.sanity_check() {
                error!("sanity check failed");
                return;
            }
            let pid = Pid::from_raw(pid.get() as i32);
            if let Err(error) = nix::sys::signal::kill(pid, signal) {
                error!("{error}");
            }
        } else {
            error!("no running process")
        }
    }

    pub fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        };
    }
}
