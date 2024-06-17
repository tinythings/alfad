use crate::config::yaml::TaskConfigYaml;

pub mod ctl;

pub trait IntoConfig {
    fn into_config(self) -> TaskConfigYaml;
}

#[macro_export]
macro_rules! builtin_fn {
    ($name:ident: $function:ident) => {
        pub struct $name;

        impl $name {
            pub fn box_fn() -> $crate::config::yaml::PayloadYaml {
                $crate::config::yaml::PayloadYaml::Builtin(Box::leak(Box::new($name)))
            }
        }


        #[async_trait::async_trait]
        impl $crate::config::payload::Runnable for $name {
            async fn run<'a>(
                &'a self,
                context: &'a RwLock<TaskContext>,
                context_map: ContextMap<'static>,
            ) -> ControlFlow<TaskState> {
                match $function(context, context_map).await {
                    Ok(_) => ControlFlow::Continue(()),
                    Err(error) => {
                        tracing::error!(%error);
                        ControlFlow::Break($crate::task::TaskState::Failed)
                    }
                }
            }
        }

    };
}