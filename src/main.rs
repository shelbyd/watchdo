use crossbeam_channel::TryRecvError;
use notify::{RecommendedWatcher, RecursiveMode, Result, Watcher};
use std::time::Duration;
use std::process::{Command, Stdio};

fn main() -> Result<()> {
    let (tx, rx) = crossbeam_channel::unbounded();

    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_secs(1))?;

    watcher.watch(".", RecursiveMode::Recursive)?;

    let mut want_to_run = false;
    let mut current_process = None;

    loop {
        match rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => Err(crossbeam_channel::RecvError)?,
            Ok(event) => {
                want_to_run = true;
                let _ = event?;
            }
        }

        if let None = &current_process {
            if want_to_run {
                current_process = Some(
                    Command::new("cargo")
                        .args(&["test"])
                        .stdout(Stdio::null())
                        .stdin(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()?,
                );
                want_to_run = false;
            }
        }

        if let Some(p) = &mut current_process {
            if let Some(exit) = p.try_wait()? {
                current_process = None;

                if exit.success() {
                    println!("Tests passed");
                } else {
                    println!("Tests failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_test() {
        std::thread::sleep(Duration::from_secs(1));
    }
}
