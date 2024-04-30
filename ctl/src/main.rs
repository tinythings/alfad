use std::{fs::OpenOptions, io::Write};

use anyhow::{Context, Result};
use clap::Parser;

use alfad::action::Action;


fn main() -> Result<()> {
    let action = Action::parse();
    OpenOptions::new()
        .write(true)
        .open("test/alfad-pipe").context("alfad pipe not found")?
        .write_all(action.to_string().as_bytes())?;
    Ok(())
}