pub mod action;
pub mod command_line;
pub mod config;
pub mod ordering;
mod perform_action;
pub mod task;
mod validate;
mod init;

use std::{env, fs::OpenOptions, io::Write, path::Path};

use anyhow::{Context, Result};
use clap::Parser;

use alfad::action::{Action, SystemCommand};

pub static VERSION: &str = "0.1";

fn main() -> Result<()> {
    let name = env::args().next().unwrap();
    let name = Path::new(&name).file_name().unwrap().to_str().unwrap();
    let action = match name {
        "alfad-ctl" => Action::parse_from(env::args()),
        "init" => {
            init::main();
            unreachable!()
        }
        _ => Action::System {
            command: SystemCommand::parse_from([String::new()].into_iter().chain(env::args())),
        },
    };
    OpenOptions::new()
        .write(true)
        .open("/run/var/alfad-ctl")
        .context("alfad pipe not found")?
        .write_all(action.to_string().as_bytes())?;
    Ok(())
}
