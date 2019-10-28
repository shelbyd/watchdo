#![cfg_attr(feature = "strict", deny(warnings))]

use colored::{ColoredString, Colorize};
use crossbeam_channel::TryRecvError;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use structopt::StructOpt;

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
    command: OsString,
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

    let mut test_command = CommandRunner::new(SubprocessExecutor::new(&options.command));
    let mut server_command = options
        .server
        .clone()
        .map(|s| CommandRunner::new(SubprocessExecutor::new(s)));

    let mut last_printed = None;
    let mut history = TestsHistory::new(Duration::from_millis(100), &options);
    history.new_file_tree();

    loop {
        loop {
            match rx.try_recv() {
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => Err(crossbeam_channel::RecvError)?,
                Ok(event) => {
                    history.new_file_tree();
                    let _ = event?;
                }
            }
        }

        if history.want_to_run() {
            test_command.run()?;
            history.run();
        }

        if let Some(output) = test_command.try_finish()? {
            if !output.success {
                eprintln!("{}", output.err);
                println!("{}", output.out);
            }
            history.finished(output);
        }

        if let Some(server_command) = server_command.as_mut() {
            history.server_try_finish(server_command.try_finish()?);

            if history.want_to_restart_server() {
                match server_command.is_running()? {
                    true => {
                        server_command.terminate()?;
                        history.server_terminated();
                    }
                    false => {
                        server_command.run()?;
                        history.server_started();
                    }
                }
            }
        }

        let width = term_size::dimensions().map(|d| d.0).unwrap_or(80);
        let to_print = history.print(width).collect::<Vec<_>>();
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

struct TestsHistory<'o> {
    history: Vec<TestState>,
    // History index the server is running at.
    server_running_at: Option<usize>,
    server_history: HashMap<usize, ServerState>,
    throttle: Duration,
    options: &'o Options,
}

impl TestsHistory<'_> {
    fn new(throttle: Duration, options: &Options) -> TestsHistory {
        TestsHistory {
            history: Vec::new(),
            server_running_at: None,
            server_history: HashMap::new(),
            throttle,
            options,
        }
    }

    fn new_file_tree(&mut self) {
        match self.history.last() {
            Some(TestState::NotRan { requested_at }) => {
                if requested_at.elapsed() <= self.throttle {
                    return;
                }
            }
            _ => {}
        }

        self.history.push(TestState::NotRan {
            requested_at: Instant::now(),
        });
    }

    fn want_to_run(&self) -> bool {
        let currently_running = self
            .history
            .iter()
            .filter(|h| *h == &TestState::Running)
            .next();

        match (currently_running, self.history.last()) {
            (None, Some(TestState::NotRan { .. })) => true,
            _ => false,
        }
    }

    fn currently_running(&mut self) -> Option<&mut TestState> {
        self.history
            .iter_mut()
            .filter(|h| *h == &TestState::Running)
            .next()
    }

    fn run(&mut self) {
        *self.history.last_mut().unwrap() = TestState::Running;
    }

    fn finished(&mut self, output: CommandOutput) {
        let running = self.currently_running().unwrap();
        *running = TestState::Completed(output);
    }

    fn want_to_restart_server(&self) -> bool {
        match (self.history.last(), self.server_running_at.as_ref()) {
            (_, Some(index)) if *index == (self.history.len() - 1) => false,
            (Some(TestState::Completed(output)), _) if output.success => true,
            _ => false,
        }
    }

    fn server_try_finish(&mut self, output: Option<CommandOutput>) {
        match (self.server_running_at, output) {
            (Some(index), Some(output)) => self.server_history_index(index).finished(output),
            (None, None) => {} // Not running and not done.
            (None, Some(_)) => unreachable!(),
            (Some(_), None) => {} // Running and not done.
        };
    }

    fn server_history_index(&mut self, index: usize) -> &mut ServerState {
        self.server_history
            .entry(index)
            .or_insert(ServerState::new())
    }

    fn server_terminated(&mut self) {
        let index = self.server_running_at.unwrap_or_else(|| unreachable!());
        self.server_history_index(index).terminated();
    }

    fn server_started(&mut self) {
        let index = self.history.len() - 1;
        self.server_running_at = Some(index);
        self.server_history_index(index).running();
    }

    fn print(&self, n: usize) -> impl Iterator<Item = ColoredString> + '_ {
        self.print_test(n).chain(self.print_server(n))
    }

    fn print_test(&self, n: usize) -> impl Iterator<Item = ColoredString> + '_ {
        let history_chars = self.history.iter().map(move |state| match state {
            TestState::NotRan { .. } => ".".normal(),
            TestState::Running => "?".black().on_yellow(),
            TestState::Completed(output) => {
                if output.success {
                    self.options.ok_str.white().on_green()
                } else {
                    "x".white().on_red()
                }
            }
        });
        let spaces = std::iter::repeat(" ".normal().on_white()).take(n);
        let whole_print = spaces.chain(history_chars);
        match whole_print.size_hint() {
            (min, Some(max)) => {
                assert_eq!(min, max);
                whole_print.skip(min - n).take(n)
            }
            _ => unreachable!(),
        }
    }

    fn print_server(&self, n: usize) -> Box<dyn Iterator<Item = ColoredString> + '_> {
        if let (true, None) = (self.server_history.is_empty(), self.server_running_at) {
            return Box::new(std::iter::empty());
        }

        let spaces = std::iter::repeat(" ".normal()).take(n);
        let chars =
            (0..self.history.len()).map(move |index| match self.server_history.get(&index) {
                None => " ".normal(),
                Some(ServerState {
                    terminated: false,
                    state: ServerCommandState::Running,
                }) => "?".black().on_yellow(),
                Some(ServerState {
                    terminated: true,
                    state: ServerCommandState::Running,
                }) => "x".black().on_yellow(),
                Some(ServerState {
                    terminated: true,
                    state: ServerCommandState::Completed(output),
                }) => {
                    if output.success {
                        self.options.ok_str.black().on_white()
                    } else {
                        "x".black().on_white()
                    }
                }
                Some(ServerState {
                    terminated: false,
                    state: ServerCommandState::Completed(output),
                }) => {
                    if output.success {
                        self.options.ok_str.white().on_red()
                    } else {
                        "x".white().on_red()
                    }
                }
            });
        let whole_print = spaces.chain(chars);
        let whole_print = match whole_print.size_hint() {
            (min, Some(max)) => {
                assert_eq!(min, max);
                whole_print.skip(min - n).take(n)
            }
            _ => unreachable!(),
        };

        let newline = std::iter::once("\n".normal());
        Box::new(newline.chain(whole_print))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TestState {
    NotRan { requested_at: Instant },
    Running,
    Completed(CommandOutput),
}

struct ServerState {
    terminated: bool,
    state: ServerCommandState,
}

impl ServerState {
    fn new() -> Self {
        ServerState {
            terminated: false,
            state: ServerCommandState::Running,
        }
    }

    fn running(&mut self) {
        self.state = ServerCommandState::Running;
    }

    fn terminated(&mut self) {
        self.terminated = true;
    }

    fn finished(&mut self, output: CommandOutput) {
        self.state = ServerCommandState::Completed(output);
    }
}

enum ServerCommandState {
    Running,
    Completed(CommandOutput),
}
