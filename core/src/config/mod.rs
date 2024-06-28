pub mod payload;
pub mod yaml;
use self::{payload::Payload, yaml::TaskConfigYaml};
use crate::{
    ordering::{construct_markers, resolve_before, sort},
    validate,
};
use serde::{Deserialize, Serialize};
use smol::stream::StreamExt;
use std::{
    error::Error,
    fmt::Debug,
    fs::{self, read_dir, OpenOptions},
    path::Path,
};
use tracing::{debug, info_span};
use tracing::{error, instrument};

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Respawn {
    /// Never retry this task (default)
    No,
    /// Restart this task up to N times
    ///
    /// N = 0, restart this task an unlimited number of times
    // TODO: Does manual restart affect the counter, if so: how
    Retry(usize),
}

impl Default for Respawn {
    fn default() -> Self {
        Self::No
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TaskConfig {
    pub name: String,
    // #[serde(default)]
    pub payload: Payload,
    // #[serde(default)]
    pub with: Vec<String>,
    // #[serde(default)]
    pub after: Vec<String>,
    // #[serde(default)]
    pub respawn: Respawn,
    pub group: Option<String>,
}

impl TaskConfig {
    pub fn new(name: String) -> Self {
        Self { name, ..Default::default() }
    }

    pub fn after(&mut self, name: &str) -> &mut Self {
        self.after.push(name.to_owned());
        self
    }
}

pub fn read_config(builtin: Vec<TaskConfigYaml>) -> Vec<TaskConfig> {
    let configs = if cfg!(debug_assertions) { "test" } else { "/etc/alfad" };
    let configs = Path::new(configs);

    match read_binary(configs.join("alfad.bin").as_path()) {
        Some(mut configs) => {
            configs.extend(builtin.into_iter().map(TaskConfigYaml::into_config).filter_map(drop_errors));
            configs
        }
        None => read_yaml_configs(configs.join("alfad.d").as_path(), builtin),
    }
}

#[instrument]
pub fn read_binary(path: &Path) -> Option<Vec<TaskConfig>> {
    let packed = fs::read(path).map_err(|error| error!("Can't find alfad.bin {error}")).ok()?;
    let (version, res) = postcard::from_bytes::<(String, Vec<_>)>(&packed).map_err(|error| error!(?error)).ok()?;
    if version == crate::VERSION {
        Some(res)
    } else {
        error!("Wrong version {} != {}", version, crate::VERSION);
        None
    }
}

pub fn read_yaml_configs(path: &Path, builtin: Vec<TaskConfigYaml>) -> Vec<TaskConfig> {
    let span = info_span!("Parsing task files");
    let _span = span.enter();
    let dir_reader = match read_dir(path) {
        Ok(rd) => rd,
        Err(error) => {
            error!("Could not read config directory {path:?}: {}", error);
            return Vec::new();
        }
    };
    let mut configs: Vec<_> = smol::block_on(async {
        smol::stream::iter(dir_reader)
            .filter_map(drop_errors)
            .map(|file| OpenOptions::new().read(true).open(file.path()))
            .filter_map(drop_errors)
            .map(serde_yaml::from_reader)
            .filter_map(drop_errors)
            .inspect(|config: &TaskConfigYaml| debug!("{config:?}"))
            .collect()
            .await
    });

    configs.extend(builtin);
    let groups = construct_markers(&configs);
    configs.extend(groups);

    #[cfg(feature = "before")]
    let configs = resolve_before(configs);

    let configs = configs.into_iter().map(TaskConfigYaml::into_config).filter_map(drop_errors).collect();

    #[cfg(feature = "validate")]
    let configs = validate::validate(configs);

    let configs = sort(configs);

    drop(_span);
    configs
}

fn drop_errors<T, E: Error>(r: Result<T, E>) -> Option<T> {
    match r {
        Ok(x) => Some(x),
        Err(error) => {
            error!("{error}");
            None
        }
    }
}
