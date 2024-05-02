
use serde::Deserialize;
pub use smol::process::Child;
use smol::process::Command;
use thiserror::Error;

#[derive(Debug, Deserialize, Default)]
pub struct CommandLines(CommandLine);

impl CommandLines {
    pub fn get(&self, index: usize) -> Option<&CommandLine> {
        if index == 0 {
            Some(&self.0)
        } else {
            None
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct CommandLine(Vec<String>);

impl CommandLine {
    pub fn to_command(&self) -> Result<Command, CommandLineError> {
        let mut args = self.0.iter();
        let program = args.next().ok_or(CommandLineError::EmptyCommand)?;
        let mut command = Command::new(program);
        command.args(args);
        Ok(command)
    }

    pub fn spawn(&self) -> Result<Child, CommandLineError> {
        Ok(self.to_command()?.spawn()?)
    }

}

#[derive(Debug, Error)]
pub enum CommandLineError {
    #[error("Empty Command")]
    EmptyCommand,
    #[error(transparent)]
    IO(#[from] smol::io::Error),
}
