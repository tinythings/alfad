
use std::fmt::Debug;

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use smallvec::SmallVec;


use crate::{
    builtin::BuiltInService, command_line, config::{Respawn, TaskConfig}
};

use super::payload::Payload;


#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum PayloadYaml {
    // #[serde(deserialize_with = "T::deserialize")]
    Service(String),
    #[serde(skip)]
    Builtin(BuiltInService),
    Marker
}

impl Default for PayloadYaml {
    fn default() -> Self {
        Self::Service(String::new())
    }
}

impl Debug for PayloadYaml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Service(arg0) => f.debug_tuple("Service").field(arg0).finish(),
            Self::Builtin(_) => f.write_str("<builtin>"),
            Self::Marker => f.write_str("<marker>")
        }
    }
}


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

#[derive(Debug, Deserialize, Default)]
pub struct TaskConfigYaml {
    pub name: String,
    #[serde(default)]
    pub cmd: PayloadYaml,
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
    #[serde(default)]
    #[serde(deserialize_with = "OneOrMany::read")]
    pub provides: Vec<String>
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
            payload: match self.cmd {
                PayloadYaml::Service(x) => x.parse()?,
                PayloadYaml::Builtin(builtin) => Payload::Builtin(builtin),
                PayloadYaml::Marker => Payload::Marker
            },
            with: self.with,
            after: self.after.into_vec(),
            respawn: self.respawn.into(),
            group: self.group,
        })
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
