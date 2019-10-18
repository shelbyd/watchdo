#![cfg_attr(feature = "strict", deny(warnings))]

use colored::{ColoredString, Colorize};
use crossbeam_channel::TryRecvError;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use structopt::StructOpt;
use subprocess::{Exec, ExitStatus, NullFile, Popen as Child, Redirection};

mod command_runner;
mod executor;

#[derive(StructOpt, Debug)]
struct Options {
    #[structopt(long, parse(from_os_str), default_value = "./")]
    watch_dir: PathBuf,

    #[structopt(parse(from_os_str))]
    command: OsString,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::from_args();

    let (tx, rx) = crossbeam_channel::unbounded();

    let mut watcher: RecommendedWatcher = Watcher::new_immediate(move |event| {
        tx.send(event).unwrap();
    })?;

    for result in ignore::WalkBuilder::new(options.watch_dir)
        .follow_links(true)
        .build()
    {
        watcher.watch(result?.path(), RecursiveMode::NonRecursive)?;
    }

    let mut last_printed = None;
    let mut history = TestsHistory::new(Duration::from_millis(100));
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

        if let None = &history.current_process() {
            if history.want_to_run() {
                history.run(
                    Exec::shell(&options.command)
                        .stdin(NullFile)
                        .stdout(Redirection::Pipe)
                        .stderr(Redirection::Pipe)
                        .popen()?,
                );
            }
        }

        if let Some(output) = history.try_finish()? {
            if !output.status.success() {
                eprintln!("{}", output.err);
                println!("{}", output.out);
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

struct TestsHistory {
    history: Vec<TestState>,
    throttle: Duration,
}

impl TestsHistory {
    fn new(throttle: Duration) -> TestsHistory {
        TestsHistory {
            history: Vec::new(),
            throttle,
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
        match self.history.last() {
            Some(TestState::NotRan { .. }) => true,
            _ => false,
        }
    }

    fn current_process(&mut self) -> Option<&mut Child> {
        self.currently_running().map(|state| match state {
            TestState::Running(child) => child,
            _ => unreachable!(),
        })
    }

    fn currently_running(&mut self) -> Option<&mut TestState> {
        self.history
            .iter_mut()
            .filter(|h| match h {
                TestState::Running(_) => true,
                _ => false,
            })
            .next()
    }

    fn run(&mut self, child: Child) {
        *self.history.last_mut().unwrap() = TestState::Running(child);
    }

    fn finished(&mut self, output: CommandOutput) -> &CommandOutput {
        let running = self.currently_running().unwrap();
        *running = TestState::Completed(output);
        match running {
            TestState::Completed(o) => o,
            _ => unreachable!(),
        }
    }

    fn try_finish(&mut self) -> Result<Option<&CommandOutput>, Box<dyn Error>> {
        if let Some(p) = self.current_process() {
            if let Some(output) = CommandOutput::from_process(p) {
                return Ok(Some(self.finished(output?)));
            }
        }
        Ok(None)
    }

    fn print(&self, n: usize) -> impl Iterator<Item = ColoredString> + '_ {
        let history_chars = self.history.iter().map(|state| match state {
            TestState::NotRan { .. } => ".".normal(),
            TestState::Running(_) => "?".black().on_yellow(),
            TestState::Completed(output) => {
                if output.status.success() {
                    "✓".white().on_green()
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
}

#[derive(Debug)]
enum TestState {
    NotRan { requested_at: Instant },
    Running(Child),
    Completed(CommandOutput),
}

#[derive(Debug, PartialEq, Eq)]
struct CommandOutput {
    status: ExitStatus,
    out: String,
    err: String,
}

impl CommandOutput {
    fn from_process(child: &mut Child) -> Option<Result<Self, Box<dyn Error>>> {
        child.poll().map(|exit| {
            let output = child.communicate(None)?;
            Ok(CommandOutput {
                status: exit,
                out: output.0.unwrap(),
                err: output.1.unwrap(),
            })
        })
    }
}
