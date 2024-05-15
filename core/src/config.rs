use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use smallvec::{smallvec, SmallVec};
use smol::stream::StreamExt;
use std::{
    error::Error,
    fs::{self, read_dir, OpenOptions},
    path::Path,
};
use tracing::{debug, info_span};

use crate::{
    command_line,
    ordering::{construct_groups, resolve_before, sort},
    task::{Respawn, TaskConfig},
    validate,
};
use tracing::{error, instrument};

#[derive(Debug, Deserialize, Serialize, Eq, Clone, Hash, PartialEq)]
#[serde(untagged)]
pub enum RespawnYaml {
    /// Never retry this task (default)
    No,
    /// Restart this task up to N times
    ///
    /// N = 0, restart this task an unlimited number of times
    // TODO: Does manual restart affect the counter, if so: how
    Retry(usize),
}

impl Default for RespawnYaml {
    fn default() -> Self {
        Self::No
    }
}

impl From<RespawnYaml> for Respawn {
    fn from(value: RespawnYaml) -> Self {
        match value {
            RespawnYaml::No => Respawn::No,
            RespawnYaml::Retry(x) => Respawn::Retry(x),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Hash, PartialEq, Eq)]
pub struct TaskConfigYaml {
    pub name: String,
    #[serde(default)]
    pub cmd: String,
    #[cfg(feature = "before")]
    #[serde(default)]
    #[serde(deserialize_with = "OneOrMany::read")]
    pub before: Vec<String>,
    #[serde(default)]
    #[serde(deserialize_with = "OneOrMany::read")]
    pub with: Vec<String>,
    #[serde(default)]
    #[serde(deserialize_with = "OneOrMany::read")]
    pub after: SmallVec<[String; 1]>,
    #[serde(default)]
    pub respawn: RespawnYaml,
    pub group: Option<String>,
}

impl TaskConfigYaml {
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

    pub fn into_config(self) -> Result<TaskConfig, command_line::CommandLineError> {
        Ok(TaskConfig {
            name: self.name,
            cmd: self.cmd.parse()?,
            with: self.with,
            after: self.after.into_vec(),
            respawn: self.respawn.into(),
            group: self.group,
        })
    }
}

#[instrument]
pub fn read_config() -> Vec<TaskConfig> {
    let configs = if cfg!(debug_assertions) {
        "test"
    } else {
        "/etc/alfad"
    };
    let configs = Path::new(configs);

    read_binary(configs.join("alfad.bin").as_path())
        .unwrap_or_else(|| read_yaml_configs(configs.join("alfad.d").as_path()))
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

#[instrument]
pub fn read_yaml_configs(path: &Path) -> Vec<TaskConfig> {
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

    configs.extend(get_built_in());
    let groups = construct_groups(&configs);
    configs.extend(groups);

    #[cfg(feature = "before")]
    let configs = resolve_before(configs);

    let configs = configs
        .into_iter()
        .map(TaskConfigYaml::into_config)
        .filter_map(drop_errors)
        .collect();

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

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum OneOrMany<One, Many> {
    /// Single value
    One(One),
    /// Array of values
    Many(Many),
}

impl<T, List> OneOrMany<T, List>
where
    T: DeserializeOwned,
    List: DeserializeOwned + FromIterator<T>,
{
    pub fn read<'de, D>(deserializer: D) -> Result<List, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::deserialize(deserializer).map(|oom| match oom {
            OneOrMany::One(one) => [one].into_iter().collect(),
            OneOrMany::Many(many) => many,
        })
    }
}

fn get_built_in() -> Vec<TaskConfigYaml> {
    vec![TaskConfigYaml {
        name: "builtin:create-alfad-ctl".to_string(),
        cmd: "mkdir -p /run/var\nmkfifo /run/var/alfad-ctl".to_string(),
        after: smallvec!["mount-sys-fs".to_owned()],
        ..Default::default()
    }]
}

#[cfg(test)]
mod test {
    use serde::Deserialize;
    use smallvec::SmallVec;

    use super::OneOrMany;

    #[test]
    fn one_or_many_from_string() {
        serde_yaml::from_str::<OneOrMany<String, Vec<String>>>("one").unwrap();
        serde_yaml::from_str::<OneOrMany<String, Vec<String>>>("[one, two, three]").unwrap();
        serde_yaml::from_str::<OneOrMany<String, SmallVec<[String; 2]>>>("one").unwrap();
        serde_yaml::from_str::<OneOrMany<String, SmallVec<[String; 2]>>>("[one, two, three]")
            .unwrap();
    }

    #[test]
    fn one_or_many_in_struct() {
        #[derive(Deserialize)]
        struct Test {
            #[serde(deserialize_with = "OneOrMany::read")]
            _after: Vec<String>,
        }
        serde_yaml::from_str::<Test>(
            r#"
        name: bar
        cmd: [echo, "hello from inside bar"]
        _after: 
            - foo
            - bar
        "#,
        )
        .unwrap();
    }
}
