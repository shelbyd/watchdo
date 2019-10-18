use crate::executor::*;
use std::error::Error;

pub struct CommandRunner<E: Executor> {
    executor: E,
    child: Option<E::Child>,
}

impl<E: Executor> CommandRunner<E> {
    pub fn new(executor: E) -> Self {
        CommandRunner {
            executor,
            child: None,
        }
    }

    pub fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.child = Some(self.executor.start()?);
        Ok(())
    }

    pub fn try_finish(&mut self) -> Result<Option<CommandOutput>, Box<dyn Error>> {
        if let Some(c) = self.child.as_mut() {
            if let Some(output) = c.poll()? {
                self.child = None;
                return Ok(Some(output));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_starts_executor() {
        let mut child = MockChild::new();
        let mut executor = MockExecutor::new();
        executor
            .expect_start()
            .times(1)
            .return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
    }

    #[test]
    fn run_try_finish_is_none() {
        let mut child = MockChild::new();
        child.expect_poll().return_once(|| Ok(None));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
        assert_eq!(runner.try_finish().unwrap(), None);
    }

    #[test]
    fn run_try_finish_is_some() {
        let mut child = MockChild::new();
        child
            .expect_poll()
            .return_once(|| Ok(Some(CommandOutput::default())));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
        assert_eq!(runner.try_finish().unwrap(), Some(CommandOutput::default()));
    }

    #[test]
    fn try_finish_no_run_is_none() {
        let mut child = MockChild::new();
        let mut executor = MockExecutor::new();
        let mut runner = CommandRunner::new(executor);

        assert_eq!(runner.try_finish().unwrap(), None);
    }

    #[test]
    fn run_try_finish_twice_forgets_child() {
        let mut child = MockChild::new();
        child
            .expect_poll()
            .times(1)
            .return_once(|| Ok(Some(CommandOutput::default())));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
        runner.try_finish();
        assert_eq!(runner.try_finish().unwrap(), None);
    }
}
