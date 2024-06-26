use std::{
    ops::ControlFlow,
    pin::{pin, Pin},
    task::Poll,
};

use crate::task::{ContextMap, ExitReason, TaskContext};
use async_trait::async_trait;
use futures::{ready, Future};
use tracing::{debug, info};

use crate::{
    config::{payload::Runnable, yaml::TaskConfigYaml},
    task::TaskState,
};

pub mod ctl;

pub trait IntoConfig {
    fn into_config(self) -> TaskConfigYaml;
}

pub struct BuiltInService {
    function: &'static (dyn Runnable + Sync + Send),
}

#[async_trait]
impl Runnable for BuiltInService {
    async fn run<'a>(
        &'a self,
        context: &'a TaskContext,
        context_map: ContextMap<'static>,
    ) -> ControlFlow<TaskState> {
        BuiltInServiceManager {
            function: pin!(self.function.run(context, context_map)),
            context,
        }
        .await
    }
}

pub struct BuiltInServiceManager<'a, T: Future<Output = ControlFlow<TaskState>>> {
    function: Pin<&'a mut T>,
    context: &'a TaskContext,
}

impl<'a, T: Future<Output = ControlFlow<TaskState>>> Future for BuiltInServiceManager<'a, T> {
    type Output = ControlFlow<TaskState>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let state = ready!(pin!(self.context.state()).poll(cx));
        debug!(name = self.context.config.name, %state);
        if state == TaskState::Terminating {
            info!(name = self.context.config.name, "Terminating");
            Poll::Ready(ControlFlow::Break(TaskState::Concluded(
                ExitReason::Terminated,
            )))
        } else {
            ready!(pin!(self.context.set_waker(cx.waker())).poll(cx));
            self.function.as_mut().poll(cx)
        }
    }
}

#[macro_export]
macro_rules! builtin_fn {
    ($name:ident: $function:ident) => {
        pub struct $name;

        impl $name {
            pub fn box_fn() -> $crate::config::yaml::PayloadYaml {
                $crate::config::yaml::PayloadYaml::Builtin($crate::builtin::BuiltInService{ function: Box::leak(Box::new($name))})
            }
        }


        #[async_trait::async_trait]
        impl $crate::config::payload::Runnable for $name {
            async fn run<'a>(
                &'a self,
                context: &'a TaskContext,
                context_map: ContextMap<'static>,
            ) -> ControlFlow<TaskState> {
                match $function(context, context_map).await {
                    Ok(_) => ControlFlow::Continue(()),
                    Err(error) => {
                        tracing::error!(%error);
                        ControlFlow::Break($crate::task::TaskState::Concluded($crate::task::ExitReason::Failed))
                    }
                }
            }
        }

    };
}
