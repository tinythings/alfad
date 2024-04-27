use std::{
    collections::HashMap,
    future::Future,
    num::NonZeroU32,
    path::Path,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll, Waker},
};

use enum_display_derive::Display;
use nix::{sys::signal::Signal, unistd::Pid};
use std::fmt::Display;
use tracing::{error, info};

use serde::Deserialize;
use smol::{
    lock::{RwLock, RwLockWriteGuard},
    process::{Child, Command},
    ready,
};

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
    No,
    Always,
    Timeout,
}

impl Default for Respawn {
    fn default() -> Self {
        Self::No
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskConfig {
    pub name: String,
    #[serde(default)]
    cmd: Vec<String>,
    #[serde(default)]
    before: Vec<String>,
    #[serde(default)]
    with: Vec<String>,
    #[serde(default)]
    after: Vec<String>,
    #[serde(default)]
    respawn: Respawn,
}

pub struct Task<'a> {
    pub state: TaskState,
    pub config: &'a TaskConfig,
    pub context: &'a HashMap<&'a str, Arc<RwLock<TaskContext>>>,
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

    pub fn new(
        config: &'a TaskConfig,
        context: &'a HashMap<&'a str, Arc<RwLock<TaskContext>>>,
    ) -> Self {
        Self {
            state: TaskState::Waiting,
            config,
            context,
            process: None,
        }
    }

    fn poll_internal(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let mut context = ready!(pin!(self.get_context_mut()).poll(cx));
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
                    ready!(pin!(self.running()).poll(cx))
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
        for name in self.config.after.iter() {
            let mut context =
                smol::block_on(async { self.context.get(name.as_str()).unwrap().write().await });
            if context.state == TaskState::Done {
                continue;
            }
            context.waiters_done.push(cx.waker().clone());
            info!("{} waiting for {name}: Done", self.config.name);

            return Poll::Pending;
        }
        for name in self.config.with.iter() {
            let mut context =
                smol::block_on(async { self.context.get(name.as_str()).unwrap().write().await });
            if context.state == TaskState::Running {
                continue;
            }
            context.waiters_running.push(cx.waker().clone());
            info!("{} waiting for {name}: Running", self.config.name);

            return Poll::Pending;
        }
        Poll::Ready(())
    }

    async fn running(&mut self) {
        if let Some(child) = self.process.as_mut() {
            match child.status().await {
                Ok(status) if status.success() => self.state = TaskState::Done,
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
            let p = Command::new(program).args(args).spawn().unwrap();
            self.get_context_mut().await.pid = NonZeroU32::new(p.id());
            self.process = Some(p);
        }
    }

    async fn get_context_mut(&mut self) -> RwLockWriteGuard<'_, TaskContext> {
        self.context
            .get(self.config.name.as_str())
            .unwrap()
            .write()
            .await
    }

    async fn propagate_state(&mut self) {
        self.trace();
        let state = self.state;
        self.get_context_mut().await.update_state(state);
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
            TaskState::Running => self.waiters_running.drain(..).for_each(|w| w.wake_by_ref()),
            TaskState::Done => self.waiters_done.drain(..).for_each(|w| w.wake_by_ref()),
            _ => {}
        }
    }

    pub fn sanity_check(&self) -> bool {
        if let Some(pid) = self.pid {
            return Path::new(&format!("/proc/{}", pid)).exists();
        }
        return false;
    }

    pub fn send_signal(&self, signal: Signal) {
        if let Some(pid) = self.pid {
            if !self.sanity_check() {
                error!("sanity check failed");
                return;
            }
            let pid = Pid::from_raw(pid.get() as i32);
            nix::sys::signal::kill(pid, signal).unwrap();
        } else {
            error!("no running process")
        }
    }

    pub fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }
}
