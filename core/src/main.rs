pub mod action;
pub mod builtin;
pub mod command_line;
pub mod config;
mod init;
pub mod ordering;
mod perform_action;
pub mod task;
mod validate;

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::Parser;

use alfad::action::{Action, SystemCommand};
use config::{read_yaml_configs, yaml::TaskConfigYaml, TaskConfig};
use itertools::Itertools;
use tracing::{Level};
use tracing_subscriber::FmtSubscriber;

use crate::builtin::{
    ctl::{CreateCtlPipe, WaitForCommands},
    IntoConfig,
};

pub static VERSION: &str = "0.1";

fn main() -> Result<()> {
    let name = env::args().next().unwrap();
    let name = Path::new(&name).file_name().unwrap().to_str().unwrap();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::TRACE)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let action = match name {
        "alfad-ctl" => Action::parse_from(env::args()),
        "alfad-compile" => return compile(),
        "init" => {
            return init::Alfad {
                builtin: get_built_in(),
            }
            .run()
        }
        _ => Action::System {
            command: SystemCommand::parse_from([String::new()].into_iter().chain(env::args())),
        },
    };

    let payload = action.to_string();

    OpenOptions::new()
        .write(true)
        .open("/run/var/alfad-ctl")
        .context("alfad pipe not found")?
        .write_all(payload.as_bytes())?;
    Ok(())
}

fn get_built_in() -> Vec<TaskConfigYaml> {
    vec![CreateCtlPipe.into_config(), WaitForCommands.into_config()]
}

#[derive(Parser)]
struct Cli {
    #[clap(default_value = "/etc/alfad")]
    target: PathBuf,
}

fn compile() -> Result<()> {
    let cli = Cli::parse();

    let builtin = get_built_in();

    let configs = (
        VERSION,
        read_yaml_configs(&cli.target.join("alfad.d"), get_built_in())
            .into_iter()
            .filter(|x| builtin.iter().all(|bi| bi.name != x.name))
            .collect_vec(),
    );

    let data = postcard::to_allocvec(&configs)?;
    let (_, _): (String, Vec<TaskConfig>) = postcard::from_bytes(data.as_ref())?;

    fs::write(
        cli.target.join("alfad.bin"),
        data,
    )?;
    Ok(())
}
