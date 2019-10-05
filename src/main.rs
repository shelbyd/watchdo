use notify::{RecommendedWatcher, RecursiveMode, Result, Watcher};
use std::time::Duration;

fn main() -> Result<()> {
    let (tx, rx) = crossbeam_channel::unbounded();

    let mut watcher: RecommendedWatcher = Watcher::new(tx, Duration::from_millis(50))?;

    watcher.watch(".", RecursiveMode::Recursive)?;

    loop {
        let event = rx.recv()??;
        dbg!(event);
    }
}
