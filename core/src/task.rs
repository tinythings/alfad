use std::{
    collections::HashMap,
    fmt::Debug,
    future::Future,
    ops::ControlFlow,
    path::Path,
    pin::{pin, Pin},
    task::{Context, Poll, Waker},
};

use strum::Display;

use nix::{sys::signal::Signal, unistd::Pid};

use tracing::{error, info, info_span, trace, warn};

use serde::Deserialize;
use smol::{lock::RwLock, ready};

use crate::{command_line::Child, config::{payload::Payload, Respawn, TaskConfig}};

pub type ContextMap<'a> = &'a HashMap<&'a str, RwLock<TaskContext>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Display)]
pub enum TaskState {
    Waiting,
    Starting,
    Running(usize),
    Done,
    Failed,
    Terminating,
    Terminated,
    Killed,
    /// Like Terminated but will not try to run again even if retries are left
    Deactivated,
}

impl TaskState {
    pub fn has_concluded(&self) -> bool {
        matches!(
            self,
            Self::Deactivated | Self::Done | Self::Failed | Self::Terminated | Self::Killed
        )
    }

    pub fn is_waiting(&self) -> bool {
        *self == Self::Waiting
    }
}

impl Default for TaskState {
    fn default() -> Self {
        Self::Waiting
    }
}

#[derive(Debug)]
pub struct Task<'a> {
    pub state: TaskState,
    old_state: TaskState,
    pub config: &'a TaskConfig,
    pub context_map: ContextMap<'static>,
    context: &'a RwLock<TaskContext>,
    // pub process: Option<Child>,
}

impl Future for Task<'_> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let p = self.poll_internal(cx);
        ready!(pin!(self.propagate_state()).poll(cx));
        p
    }
}

macro_rules! wait_for {
    ($s:path, $queue:ident, $state:pat, $cx:ident, $dsp:literal) => {{
        let inner = |cx: &mut Context| -> Poll<()> {
            for name in $s.config.$queue.iter() {
                if let Some(other) = $s.context_map.get(name.as_str()) {
                    let r = smol::block_on(async {
                        let mut context = other.write().await;
                        if matches!(context.state, $state) {
                            trace!("'{name}' is ready");
                            true
                        } else {
                            context.$queue.push(cx.waker().clone());
                            false
                        }
                    });
                    if r {
                        continue;
                    } else {
                        info!("'{}' waiting for '{name}' to be {}", $s.config.name, $dsp);
                        return Poll::Pending;
                    }
                } else {
                    warn!(
                        "'{}' is waiting for '{}', which does not exist, and will never run",
                        $s.config.name, name
                    );
                    return Poll::Pending;
                }
            }
            Poll::Ready(())
        };

        inner($cx)
    }};
}

impl<'a> Task<'a> {
    pub fn trace(&self) {
        info!("{} is {:?}", self.config.name, &self.state);
    }

    pub fn spawn(config: &'static TaskConfig, context_map: ContextMap<'static>) {
        if matches!(config.payload, Payload::Service(_) | Payload::Builtin(_)) {
            info!("Spawning {}", config.name);
        }
        smol::spawn(async move { Self::new(config, context_map).await }).detach()
    }

    pub fn new(
        config: &'a TaskConfig,
        context_map: ContextMap<'static>,
    ) -> Self {
        Self {
            state: TaskState::Waiting,
            old_state: TaskState::Waiting,
            config,
            context_map,
            context: context_map
                .get(config.name.as_str())
                .expect("generated from the same list and must thus be in the context_map")
        }
    }

    fn poll_internal(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        let _s = info_span!("Driving", task = self.config.name);
        let _s = _s.enter();

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
            trace!(state = ?self.state);
            use TaskState as S;
            self.state = match self.state {
                S::Waiting => {
                    ready!(self.wait_for_dependencies(cx))
                }
                S::Starting => S::Running(0),
                S::Running(x) => {
                    ready!(pin!(self.running(x)).poll(cx))
                }
                S::Terminating => {
                    ready!(pin!(self.wait_for_terminate()).poll(cx))
                }
                state @ (S::Failed | S::Terminated) => {
                    match ready!(pin!(self.respawn(state)).poll(cx)) {
                        ControlFlow::Continue(state) => state,
                        ControlFlow::Break(_) => {
                            return Poll::Pending;
                        }
                    }
                }
                _ => return Poll::Pending,
            }
        }
    }

    async fn respawn(&mut self, state: TaskState) -> ControlFlow<TaskState, TaskState> {
        match self.config.respawn {
            Respawn::No => ControlFlow::Break(state),
            Respawn::Retry(amount) => self.respawn_inner(amount).await,
        }
    }

    async fn respawn_inner(&mut self, amount: usize) -> ControlFlow<TaskState, TaskState> {
        if amount != 0 {
            let attempts = self.context.read().await.respawn_attempts;
            if attempts >= amount {
                info!("Deactivating {task}", task = self.config.name);
                return ControlFlow::Break(TaskState::Deactivated);
            }
            self.context.write().await.respawn_attempts += 1;
        }
        info!("Restarting {task}", task = self.config.name);
        ControlFlow::Continue(TaskState::Waiting)
    }

    fn wait_for_dependencies(&mut self, cx: &mut Context<'_>) -> Poll<TaskState> {
        ready!(wait_for!(self, after, TaskState::Done, cx, "Done"));
        ready!(wait_for!(self, with, TaskState::Running(_), cx, "Running"));
        Poll::Ready(TaskState::Starting)
    }

    async fn running(&mut self, x: usize) -> TaskState {
        let _s = info_span!("Running", task = self.config.name);
        let _s = _s.enter();

        let payload = &self.config.payload;
        // let context = &mut self.context;

        match payload.run(x, self.context, self.context_map).await {
            ControlFlow::Break(state) => state,
            ControlFlow::Continue(_) => TaskState::Running(x + 1)
        }
    }

    async fn wait_for_terminate(&mut self) -> TaskState {
        self.context.write().await.wait_for_terminate().await
    }

    async fn propagate_state(&mut self) {
        if self.state == self.old_state {
            return;
        }
        self.old_state = self.state;
        self.trace();
        let state = self.state;
        self.context.write().await.update_state(state);
    }
}

#[derive(Debug, Default)]
pub struct TaskContext {
    state: TaskState,
    with: Vec<Waker>,
    after: Vec<Waker>,
    pub child: Option<Child>,
    respawn_attempts: usize,
    /// used to wake this task from the outside
    waker: Option<Waker>,
}

impl TaskContext {
    pub fn update_state(&mut self, state: TaskState) {
        self.state = state;
        match self.state {
            TaskState::Running(_) => self.with.drain(..).for_each(|w| w.wake()),
            TaskState::Done => self.after.drain(..).for_each(|w| w.wake()),
            _ => {}
        }
    }

    pub fn state(&self) -> TaskState {
        self.state
    }

    pub fn sanity_check(&self) -> bool {
        if let Some(ref pid) = self.child {
            Path::new(&format!("/proc/{}", pid.0.id())).exists()
        } else {
            false
        }
    }

    pub fn send_signal(&self, signal: Signal) {
        if let Some(ref child) = self.child {
            if !self.sanity_check() {
                error!("sanity check failed");
                return;
            }
            let pid = Pid::from_raw(child.id() as i32);
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

    pub async fn wait_for_terminate(&mut self) -> TaskState {
        if let Some(child) = self.child.as_mut() {
            info!("Killing {:?}", child.id());
            child.status().await.ok();
            self.state = TaskState::Terminated;
            self.child = None;
        }
        TaskState::Terminated
    }
}
