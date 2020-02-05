use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::time::Duration;
use subprocess::{Exec, NullFile, Redirection};

#[cfg_attr(test, mockall::automock)]
pub trait Child {
    fn poll(&mut self) -> Result<Option<CommandOutput>, Box<dyn Error>>;
    fn terminate(&mut self) -> Result<(), Box<dyn Error>>;
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
                let output = self
                    .communicate_start(None)
                    .limit_time(Duration::from_millis(100))
                    .read_string();

                let error = match output {
                    Ok((Some(out), Some(err))) => {
                        return Ok(Some(CommandOutput {
                            success: exit.success(),
                            out,
                            err,
                        }));
                    }
                    Ok((None, _)) | Ok((_, None)) => unreachable!(),
                    Err(e) => e,
                };

                if error.kind() != std::io::ErrorKind::TimedOut {
                    return Err(Box::new(error));
                }

                let output = error.capture;
                Ok(Some(CommandOutput {
                    success: exit.success(),
                    out: std::string::String::from_utf8(output.0.unwrap())?,
                    err: std::string::String::from_utf8(output.1.unwrap())?,
                }))

            }
        }
    }

    fn terminate(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(subprocess::Popen::terminate(self)?)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub out: String,
    pub err: String,
}
