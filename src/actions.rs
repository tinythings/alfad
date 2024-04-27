use crate::StatusMap;
use nix::sys::signal::Signal;
use tracing::error;

pub async fn perform(s: &str, context: &StatusMap<'_>) {
    if let Some((action, payload)) = s.split_once(" ") {
        match action {
            "kill" => {
                let mut context = context.get(payload).unwrap().write().await;
                context.send_signal(Signal::SIGTERM);
                context.update_state(crate::task::TaskState::Terminating)
            }
            "restart" => {
                let mut context = context.get(payload).unwrap().write().await;
                context.send_signal(Signal::SIGTERM);
                context.update_state(crate::task::TaskState::Waiting);
                context.wake();
            }
            "start" => {
                let mut context = context.get(payload).unwrap().write().await;
                context.update_state(crate::task::TaskState::Waiting);
                context.wake();
            }

            _ => error!(error = "Unknown action", action, payload),
        }
    }
}
