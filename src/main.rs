#![cfg_attr(feature = "strict", deny(warnings))]

use colored::{ColoredString, Colorize};
use crossbeam_channel::TryRecvError;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use structopt::StructOpt;

mod command_history;
use self::command_history::*;

mod command_runner;
use self::command_runner::*;

mod executor;
use self::executor::*;

#[derive(StructOpt, Debug)]
struct Options {
    #[structopt(long, parse(from_os_str), default_value = "./")]
    watch_dir: PathBuf,

    #[structopt(long, parse(from_os_str))]
    server: Option<OsString>,

    #[structopt(long, default_value = "✓")]
    ok_str: String,

    #[structopt(parse(from_os_str))]
    command: Vec<OsString>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::from_args();

    let (tx, rx) = crossbeam_channel::unbounded();

    let mut watcher: RecommendedWatcher = Watcher::new_immediate(move |event| {
        tx.send(event).unwrap();
    })?;

    for result in ignore::WalkBuilder::new(options.watch_dir.clone())
        .follow_links(true)
        .build()
    {
        watcher.watch(result?.path(), RecursiveMode::NonRecursive)?;
    }

    let mut commands = Commands::new(&options.command, options.server);
    commands.request_run();

    let mut last_printed = None;
    loop {
        loop {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => Err(crossbeam_channel::RecvError)?,
                Ok(event) => {
                    commands.request_run();
                    let _ = event?;
                }
            }
        }

        commands.tick(|output| {
            eprintln!("{}", output.err);
            println!("{}", output.out);
        })?;

        let width = term_size::dimensions().map(|d| d.0).unwrap_or(80);
        let to_print = commands.print(width, &options.ok_str);
        if last_printed.as_ref() != Some(&to_print) {
            for p in to_print.iter() {
                print!("{}", p);
            }
            println!("");

            last_printed = Some(to_print);
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Commands {
    last_request: Option<Instant>,
    debounce: Duration,
    tests: Vec<CommandHistory<SubprocessExecutor>>,
    server: Option<CommandHistory<SubprocessExecutor>>,
}

impl Commands {
    fn new(tests: &[OsString], server: Option<OsString>) -> Self {
        let tests = tests
            .iter()
            .map(|c| CommandHistory::new(CommandRunner::new(SubprocessExecutor::new(c))))
            .collect();
        let server =
            server.map(|s| CommandHistory::new(CommandRunner::new(SubprocessExecutor::new(s))));
        Commands {
            last_request: None,
            debounce: Duration::from_millis(100),
            tests,
            server,
        }
    }

    fn request_run(&mut self) {
        match self.last_request {
            None => {
                self.last_request = Some(Instant::now());
            }
            Some(t) => {
                self.last_request = Some(Instant::now());
                if t.elapsed() <= self.debounce {
                    return;
                }
            }
        }

        for command in self.commands_mut() {
            command.request_run();
        }
    }

    fn commands(&self) -> impl Iterator<Item = &CommandHistory<SubprocessExecutor>> {
        self.tests.iter().chain(self.server.iter())
    }

    fn commands_mut(&mut self) -> impl Iterator<Item = &mut CommandHistory<SubprocessExecutor>> {
        self.tests.iter_mut().chain(self.server.iter_mut())
    }

    fn tick(
        &mut self,
        mut print_output: impl FnMut(&CommandOutput) -> (),
    ) -> Result<(), Box<dyn Error>> {
        for test in self.tests.iter_mut() {
            if let Some(output) = test.try_finish()? {
                if !output.success {
                    print_output(output);
                }
            }
        }

        for test in self.tests.iter_mut() {
            test.run_if_needed()?;
            if !Self::last_success(test) {
                break;
            }
        }

        if let Some(server_history) = self.server.as_mut() {
            if let Some(output) = server_history.try_finish()? {
                print_output(output);
            }

            if server_history.has_outstanding_request() {
                let all_tests_succeeded = self.tests.iter().all(Self::last_success);
                if all_tests_succeeded {
                    server_history.restart()?;
                }
            }
        }

        Ok(())
    }

    fn last_success(h: &CommandHistory<SubprocessExecutor>) -> bool {
        match h.last() {
            Some(CommandState::Completed(CommandOutput { success: true, .. })) => true,
            _ => false,
        }
    }

    fn print(&self, width: usize, ok_str: &str) -> Vec<ColoredString> {
        self.commands()
            .flat_map(|c| print(c, width, &ok_str))
            .collect()
    }
}

fn print<'c, E: Executor>(
    command_history: &'c CommandHistory<E>,
    width: usize,
    ok_str: &'c str,
) -> impl Iterator<Item = ColoredString> + 'c {
    let chars = command_history.iter().map(move |state| match state {
        CommandState::Requested => ".".normal(),
        CommandState::Running => "?".black().on_yellow(),
        CommandState::Completed(output) => {
            if output.success {
                ok_str.white().on_green()
            } else {
                "x".white().on_red()
            }
        }
        CommandState::Terminated(output) => {
            if output.success {
                ok_str.black().on_white()
            } else {
                "x".black().on_white()
            }
        }
    });
    let spaces = std::iter::repeat(" ".normal().on_white()).take(width);
    let whole_print = spaces.chain(chars);
    match whole_print.size_hint() {
        (min, Some(max)) => {
            assert_eq!(min, max);
            Box::new(whole_print.skip(min - width).take(width))
        }
        _ => unreachable!(),
    }
}
