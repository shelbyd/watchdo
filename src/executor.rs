use std::error::Error;
use std::ffi::{OsStr, OsString};
use subprocess::{Exec, NullFile, Redirection};

#[cfg_attr(test, mockall::automock)]
pub trait Child {
    fn poll(&mut self) -> Result<Option<CommandOutput>, Box<dyn Error>>;
}

#[cfg_attr(test, mockall::automock(type Child=MockChild;))]
pub trait Executor {
    type Child: Child;

    fn start(&mut self) -> Result<Self::Child, Box<dyn Error>>;
}

pub struct SubprocessExecutor {
    command: OsString,
}

impl SubprocessExecutor {
    pub fn new(command: impl AsRef<OsStr>) -> Self {
        SubprocessExecutor {
            command: command.as_ref().to_owned(),
        }
    }
}

impl Executor for SubprocessExecutor {
    type Child = subprocess::Popen;

    fn start(&mut self) -> Result<Self::Child, Box<dyn Error>> {
        Ok(Exec::shell(&self.command)
            .stdin(NullFile)
            .stdout(Redirection::Pipe)
            .stderr(Redirection::Pipe)
            .popen()?)
    }
}

impl Child for subprocess::Popen {
    fn poll(&mut self) -> Result<Option<CommandOutput>, Box<dyn Error>> {
        match subprocess::Popen::poll(self) {
            None => Ok(None),
            Some(exit) => {
                let output = self.communicate(None)?;
                Ok(Some(CommandOutput {
                    success: exit.success(),
                    out: output.0.unwrap(),
                    err: output.1.unwrap(),
                }))
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub out: String,
    pub err: String,
}
