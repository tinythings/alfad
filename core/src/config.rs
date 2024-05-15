use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use smallvec::{smallvec, Array, SmallVec};
use smol::stream::StreamExt;
use std::{
    error::Error, fs::{read_dir, OpenOptions}, str::FromStr
};
use tracing::{debug, info_span};

use crate::{
    command_line::CommandLines, ordering::{construct_groups, resolve_before}, task::TaskConfig, validate
};
use tracing::{error, instrument};

#[instrument]
pub fn read_config() -> Vec<TaskConfig> {
    let span = info_span!("Parsing task files");
    let _span = span.enter();
    let dir = if cfg!(debug_assertions) {
        "test/alfad.d"
    } else {
        // "test/alfad.d"
        "/etc/alfad/alfad.d"
    };
    
    let dir_reader = match read_dir(dir) {
        Ok(rd) => rd,
        Err(error) => {
            error!("Could not read config directory {dir:?}: {}", error);
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
            .inspect(|config: &TaskConfig| debug!("{config:?}"))
            .collect()
            .await
    });

    configs.extend(get_built_in());
    let groups = construct_groups(&configs);
    configs.extend(groups);

    #[cfg(feature = "before")]
    let configs = resolve_before(configs);

    #[cfg(feature = "validate")]
    let configs = validate::validate(configs);

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
pub enum OneOrMany<T, List> {
    /// Single value
    One(T),
    /// Array of values
    List(List),
}

impl<X: DeserializeOwned, Y: DeserializeOwned + From<OneOrMany<X, Y>>> OneOrMany<X, Y> {
    pub fn read<'de, D>(deserializer: D) -> Result<Y, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::deserialize(deserializer).map(Into::into)
    }
}

impl<T> From<OneOrMany<T, Vec<T>>> for Vec<T> {
    fn from(from: OneOrMany<T, Vec<T>>) -> Self {
        match from {
            OneOrMany::One(val) => vec![val],
            OneOrMany::List(vec) => vec,
        }
    }
}

impl<T, const SIZE: usize> From<OneOrMany<T, SmallVec<[T; SIZE]>>> for SmallVec<[T; SIZE]>
where
    [T; SIZE]: Array<Item = T>,
{
    fn from(from: OneOrMany<T, SmallVec<[T; SIZE]>>) -> Self {
        match from {
            OneOrMany::One(val) => smallvec![val],
            OneOrMany::List(vec) => vec,
        }
    }
}

fn get_built_in() -> Vec<TaskConfig> {
    vec![TaskConfig {
        name: "builtin:create-alfad-ctl".to_string(),
        cmd: CommandLines::from_str("mkdir -p /run/var\nmkfifo /run/var/alfad-ctl").unwrap(),
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
