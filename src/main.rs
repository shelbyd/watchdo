#![cfg_attr(feature = "strict", deny(warnings))]

use colored::{ColoredString, Colorize};
use crossbeam_channel::TryRecvError;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
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

        if let Some(output) = commands.test.try_finish()? {
            if !output.success {
                eprintln!("{}", output.err);
                println!("{}", output.out);
            }
        }

        commands.test.run_if_needed()?;

        if let Some(server_history) = commands.server.as_mut() {
            if let Some(output) = server_history.try_finish()? {
                eprintln!("{}", output.err);
                println!("{}", output.out);
            }

            if server_history.has_outstanding_request() {
                let previous_test_succeeded = match commands.test.last() {
                    Some(CommandState::Completed(CommandOutput { success: true, .. })) => true,
                    _ => false,
                };
                if previous_test_succeeded {
                    server_history.restart()?;
                }
            }
        }

        let width = term_size::dimensions().map(|d| d.0).unwrap_or(80);
        let mut to_print = print(&commands.test, width, &options.ok_str).collect::<Vec<_>>();
        if let Some(server_history) = &commands.server {
            to_print.extend(print(&server_history, width, &options.ok_str));
        }
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

fn print<'c, E: Executor>(
    command_history: &'c CommandHistory<E>,
    width: usize,
    ok_str: &'c str,
) -> impl Iterator<Item = ColoredString> + 'c {
    let chars = command_history.iter().map(move |state| match state {
        CommandState::Requested(_) => ".".normal(),
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

struct Commands {
    test: CommandHistory<SubprocessExecutor>,
    server: Option<CommandHistory<SubprocessExecutor>>,
}

impl Commands {
    fn new(test: &OsString, server: Option<OsString>) -> Self {
        let test = CommandHistory::new(
            CommandRunner::new(SubprocessExecutor::new(test)),
            Duration::from_millis(100),
        );
        let server = server.map(|s| {
            CommandHistory::new(
                CommandRunner::new(SubprocessExecutor::new(s)),
                Duration::from_millis(100),
            )
        });
        Commands { test, server }
    }

    fn request_run(&mut self) {
        self.test.request_run();
        if let Some(h) = &mut self.server {
            h.request_run();
        }
    }
}
