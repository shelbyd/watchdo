use crate::executor::*;
use std::error::Error;

pub struct CommandRunner<E: Executor> {
    executor: E,
    child: Option<E::Child>,
    child_output: Option<CommandOutput>,
}

impl<E: Executor> CommandRunner<E> {
    pub fn new(executor: E) -> Self {
        CommandRunner {
            executor,
            child: None,
            child_output: None,
        }
    }

    pub fn run(&mut self) -> Result<(), Box<dyn Error>> {
        self.child = Some(self.executor.start()?);
        Ok(())
    }

    pub fn try_finish(&mut self) -> Result<Option<CommandOutput>, Box<dyn Error>> {
        if self.is_running()? {
            Ok(None)
        } else {
            Ok(self.child_output.take())
        }
    }

    pub fn is_running(&mut self) -> Result<bool, Box<dyn Error>> {
        match self.child.as_mut().map(|c| c.poll()) {
            Some(Ok(Some(output))) => {
                self.child = None;
                self.child_output = Some(output);
                Ok(false)
            }
            Some(Ok(None)) => Ok(true),
            Some(Err(e)) => Err(e),
            None => Ok(false),
        }
    }

    pub fn terminate(&mut self) -> Result<(), Box<dyn Error>> {
        self.child.as_mut().map(|c| c.terminate()).unwrap_or(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_starts_executor() {
        let child = MockChild::new();
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
        let executor = MockExecutor::new();
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

    #[test]
    fn run_is_running() {
        let mut child = MockChild::new();
        child.expect_poll().return_once(|| Ok(None));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();

        assert_eq!(runner.is_running().unwrap(), true);
    }

    #[test]
    fn nothing_is_not_running() {
        let executor = MockExecutor::new();

        let mut runner = CommandRunner::new(executor);

        assert_eq!(runner.is_running().unwrap(), false);
    }

    #[test]
    fn terminated_child_is_not_running() {
        let mut child = MockChild::new();
        child
            .expect_poll()
            .return_once(|| Ok(Some(CommandOutput::default())));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
        assert_eq!(runner.is_running().unwrap(), false);
    }

    #[test]
    fn terminated_child_is_running_can_get_output_from_try_finish() {
        let mut child = MockChild::new();
        child
            .expect_poll()
            .return_once(|| Ok(Some(CommandOutput::default())));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
        runner.is_running();

        assert_eq!(runner.try_finish().unwrap(), Some(CommandOutput::default()));
    }

    #[test]
    fn terminate_terminates_a_child_process() {
        let mut child = MockChild::new();
        child.expect_terminate().times(1).return_once(|| Ok(()));

        let mut executor = MockExecutor::new();
        executor.expect_start().return_once(move || Ok(child));

        let mut runner = CommandRunner::new(executor);

        runner.run();
        runner.terminate();
    }
}
