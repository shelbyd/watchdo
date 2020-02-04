`watchdo` runs commands when watched files change. It's great for TDD workflows and/or reloading servers when sources change.

## Install

Install with cargo:

```
cargo install watchdo
```

## Usage

Run tests when source files change:

```
watchdo 'cargo test --color=always'
```

Most commands don't emit colors when writing to a program. They'll often have options to include colors anyway.

Run longer tests only after the previous ones pass.

```
watchdo 'cargo test' 'cargo test --features="integration"' 'cargo test --features="end2end"'
```

Run a server with the latest passing version.

```
watchdo 'cargo test' --server='cargo run'
```

## Notes

`watchdo` doesn't do anything special with the file system.
It assumes all your commands run based on the state of the filesystem when the command started.
If a test passes or fails incorrectly because the filesystem changed since it started, another run will start soon.

`watchdo` ignores files ignored by version control according to the `ignore` crate.
You can specify the sub-directory to watch with `--watch-dir`.
