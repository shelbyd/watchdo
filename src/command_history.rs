use crate::command_runner::*;
use crate::executor::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct CommandHistory<E: Executor> {
    runner: CommandRunner<E>,
    history: Vec<CommandState>,
    // True if the last run was explicitly terminated.
    terminated: bool,
}

impl<E: Executor> CommandHistory<E> {
    pub fn new(runner: CommandRunner<E>) -> Self {
        Self {
            runner,
            history: Vec::new(),
            terminated: false,
        }
    }

    pub fn request_run(&mut self) {
        self.history.push(CommandState::Requested);
    }

    pub fn run_if_needed(&mut self) -> Result<()> {
        if self.is_running()? {
            return Ok(());
        }

        match self.history.last_mut() {
            Some(s @ CommandState::Requested) => {
                *s = CommandState::Running;
                self.runner.run()
            }
            _ => Ok(()),
        }
    }

    fn is_running(&mut self) -> Result<bool> {
        if self.runner.is_running()? {
            return Ok(true);
        }

        self.try_finish()?;
        Ok(false)
    }

    pub fn try_finish(&mut self) -> Result<Option<&CommandOutput>> {
        let output = match self.runner.try_finish()? {
            Some(output) => output,
            None => return Ok(None),
        };

        let running = self
            .history
            .iter_mut()
            .rfind(|h| *h == &CommandState::Running)
            .unwrap();

        if self.terminated {
            *running = CommandState::Terminated(output);
        } else {
            *running = CommandState::Completed(output);
        }

        match running {
            CommandState::Completed(output) => Ok(Some(&*output)),
            CommandState::Terminated(output) => Ok(Some(&*output)),
            _ => unreachable!(),
        }
    }

    pub fn has_outstanding_request(&self) -> bool {
        match self.history.last() {
            Some(CommandState::Requested) => true,
            _ => false,
        }
    }

    pub fn last(&self) -> Option<&CommandState> {
        self.history.last()
    }

    pub fn restart(&mut self) -> Result<()> {
        if self.is_running()? {
            if self.terminated {
                // Wait for graceful shutdown.
                return Ok(());
            }

            self.terminated = true;
            self.runner.terminate()?;
        } else {
            self.runner.run()?;
        }
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandState> {
        self.history.iter()
    }
}

#[derive(PartialEq, Eq)]
pub enum CommandState {
    Requested,
    Running,
    Completed(CommandOutput),
    Terminated(CommandOutput),
}
