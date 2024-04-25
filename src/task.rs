use std::{
    collections::HashMap,
    future::Future,
    pin::{pin, Pin},
    sync::Arc,
    task::{Context, Poll, Waker},
};

use enum_display_derive::Display;
use std::fmt::Display;

use serde::Deserialize;
use smol::{
    lock::RwLock,
    process::{Child, Command},
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Display)]
pub enum TaskState {
    Uninit,
    Waiting,
    Starting,
    Running,
    Done,
    Failed,
}

impl Default for TaskState {
    fn default() -> Self {
        Self::Uninit
    }
}

#[derive(Debug, Deserialize)]
pub enum Respawn {
    No,
    Always,
    Timeout
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
    depends: HashMap<String, TaskState>,
    #[serde(default)]
    _respawn: Respawn
}

pub struct Task<'a> {
    pub state: TaskState,
    pub config: &'a TaskConfig,
    pub status: &'a HashMap<&'a str, Arc<RwLock<TaskState>>>,
    pub waiting: &'a HashMap<&'a str, Arc<RwLock<Vec<Waker>>>>,
    pub process: Option<Child>, // handle: Arc<RwLock<AtomicBool>>
}

impl Future for Task<'_> {
    type Output = TaskState;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("{} is {:?}", self.config.name, &self.state);

        if self.state == TaskState::Waiting {
            if self.as_mut().wait_for_dependencies(cx).is_pending() {
                return Poll::Pending;
            }
            self.state = TaskState::Starting
        }
        if self.state == TaskState::Starting {
            let c = pin!(self.perform()).poll(cx);
            if let Poll::Ready(child) = c {
                self.process = child;
                self.state = TaskState::Running
            } else {
                return Poll::Pending;
            }
        }
        if self.state == TaskState::Running {
            if let Some(child) = self.process.as_mut() {
                if let Poll::Ready(st) = pin!(child.status()).poll(cx) {
                    if st.unwrap().success() {
                        self.state = TaskState::Done;
                    } else {
                        self.state = TaskState::Failed;
                    }
                    smol::block_on(async { self.mark_self_done().await });
                } else {
                    return Poll::Pending;
                }
            } else {
                self.state = TaskState::Done;
            }
        }
        println!("{} is {}", self.config.name, self.state);
        Poll::Ready(self.state.clone())
    }
}

impl<'a> Task<'a> {
    pub fn new(
        config: &'a TaskConfig,
        status: &'a HashMap<&'a str, Arc<RwLock<TaskState>>>,
        waiting: &'a HashMap<&'a str, Arc<RwLock<Vec<Waker>>>>,
    ) -> Self {
        Self {
            state: TaskState::Waiting,
            config,
            status,
            waiting,
            process: None,
        }
    }

    fn wait_for_dependencies(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        for (name, state) in self.config.depends.iter() {
            let r = smol::block_on(async { self.status.get(name.as_str()).unwrap().read().await });
            if *r == *state {
                continue;
            }
            let mut w =
                smol::block_on(async { self.waiting.get(name.as_str()).unwrap().write().await });
            println!("{} waiting on \"{name}: {state}\"", self.config.name);
            w.push(cx.waker().clone());

            return Poll::Pending;
        }
        Poll::Ready(())
    }

    async fn perform(&self) -> Option<Child> {
        let mut args = self.config.cmd.iter();
        if let Some(program) = args.next() {
            Some(Command::new(program).args(args).spawn().unwrap())
        } else {
            Default::default()
        }
    }

    async fn mark_self_done(&mut self) {
        let rw = self.status.get(self.config.name.as_str()).unwrap().clone();
        *rw.write().await = self.state.clone();

        let rw = self.waiting.get(self.config.name.as_str()).unwrap().clone();
        for w in rw.write().await.iter_mut() {
            w.wake_by_ref();
        }
    }
}
