pub mod action;
pub mod builtin;
pub mod command_line;
pub mod config;
mod init;
pub mod ordering;
mod perform_action;
pub mod task;
mod validate;

use crate::builtin::{
    ctl::{CreateCtlPipe, WaitForCommands},
    IntoConfig,
};
use alfad::{
    action::{Action, SystemCommand},
    def::{APLT_COMPILE, APLT_CTL, APLT_INIT, DIR_CFG, DIR_CFG_D, DIR_RUN, FILE_CFG_BT},
};
use anyhow::{Context, Result};
use clap::Parser;
use config::{read_yaml_configs, yaml::TaskConfigYaml, TaskConfig};
use itertools::Itertools;
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

pub static VERSION: &str = "0.1";

fn main() -> Result<()> {
    let name = env::args().next().unwrap();
    let name = Path::new(&name).file_name().unwrap().to_str().unwrap();

    tracing::subscriber::set_global_default(FmtSubscriber::builder().with_max_level(Level::TRACE).finish())
        .expect("setting default subscriber failed");

    let action = match name {
        APLT_CTL => Action::parse_from(env::args()),
        APLT_COMPILE => return compile(),
        APLT_INIT => return init::Alfad { builtin: get_built_in() }.run(),
        _ => Action::System { command: SystemCommand::parse_from([String::new()].into_iter().chain(env::args())) },
    };

    OpenOptions::new()
        .write(true)
        .open(PathBuf::from(DIR_RUN).join(APLT_CTL))
        .context("alfad communication socket not found")?
        .write_all(action.to_string().as_bytes())?;
    Ok(())
}

fn get_built_in() -> Vec<TaskConfigYaml> {
    vec![CreateCtlPipe.into_config(), WaitForCommands.into_config()]
}

/// Byte-compile configuration into a cache file for faster load.
/// NOTE: Optional operation.
fn compile() -> Result<()> {
    let tgt = PathBuf::from(DIR_CFG);
    let data = postcard::to_allocvec(&(
        VERSION,
        read_yaml_configs(&PathBuf::from(DIR_CFG_D), get_built_in())
            .into_iter()
            .filter(|x| get_built_in().iter().all(|bi| bi.name != x.name))
            .collect_vec(),
    ))?;
    let (_, _): (String, Vec<TaskConfig>) = postcard::from_bytes(data.as_ref())?;

    fs::write(tgt.join(FILE_CFG_BT), data)?;
    Ok(())
}
