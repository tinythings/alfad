mod task;
mod validate;

use std::{
    collections::HashMap, fs::{read_dir, OpenOptions}, sync::Arc, task::Waker
};

use smol::lock::RwLock;
use task::{Task, TaskConfig};

use crate::task::TaskState;

type StatusMap<'a> = &'static HashMap<&'a str, Arc<RwLock<TaskState>>>;
type WaitingList<'a> = &'static HashMap<&'a str, Arc<RwLock<Vec<Waker>>>>;

#[allow(dead_code)]
static VERSION: &str = "0.1";

fn main() {
    let configs = read_config();
    #[cfg(feature = "validate")]
    let configs = validate::validate(configs);

    let configs = Box::leak(Box::new(configs));
    let status: StatusMap = Box::leak(Box::new(
        configs
            .iter()
            .map(|config| (config.name.as_str(), Default::default()))
            .collect(),
    ));

    let waiting: WaitingList = Box::leak(Box::new(
        configs
            .iter()
            .map(|config| (config.name.as_str(), Default::default()))
            .collect(),
    ));

    smol::block_on(async {
        for config in configs.iter() {
            smol::spawn(async move {
                Task::new(config, status, waiting).await;
            })
            .detach();
        }
        smol::Timer::never().await
    });
    
}

fn read_config() -> Vec<TaskConfig> {
    let dir = if cfg!(profile = "release") { "/etc/slimit/slimit.d"} else { "test/slimit.d"};
    read_dir(dir)
        .unwrap()
        .map(|file| {
            serde_yaml::from_reader(
                OpenOptions::new()
                    .read(true)
                    .open(file.unwrap().path())
                    .unwrap(),
            )
            .unwrap()
        })
        .collect()
}
