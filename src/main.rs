#[macro_use]
extern crate clap;
extern crate same_file;
extern crate sha1;
extern crate threadpool;
extern crate walkdir;

use same_file::Handle;
use sha1::Sha1;
use std::{
    env,
    fs::File,
    io::{self, Read, StdoutLock, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
};
use threadpool::ThreadPool;

mod cli;

enum Work {
    Directory { tx: Sender<Work>, path: PathBuf },
    Hashed { hash: String, path: PathBuf },
}

enum Output<'a> {
    File(File),
    Stdout(StdoutLock<'a>),
}

enum OutputType {
    File(PathBuf),
    Stdout,
}

fn main() {
    let stdout = io::stdout();
    let args = match cli::handle_args() {
        Some(args) => args,
        None => {
            cli::print_help();
            return;
        }
    };

    let (mut output, output_type) = if let Some(out) = args.output {
        (
            Output::File(File::create(&out).expect("Could not create output file")),
            OutputType::File(out),
        )
    } else {
        (Output::Stdout(stdout.lock()), OutputType::Stdout)
    };

    let pool = threadpool::Builder::new().build();
    let working_dir = env::current_dir().expect("Could not get working directory");
    let rx = start_iter(working_dir, output_type, &pool);

    while let Ok(result) = rx.recv() {
        match result {
            Work::Directory { tx, path } => {
                pool.execute(move || {
                    let hash = match hash(&path) {
                        Some(h) => h,
                        None => return,
                    };

                    tx.send(Work::Hashed { path, hash })
                        .expect("Could not signal pool");
                });
            }
            Work::Hashed { hash, path } => print(&mut output, &hash, &path),
        }
    }
}

fn print(output: &mut Output, hash: &str, path: &Path) {
    match output {
        Output::File(file) => {
            writeln!(file, "{} {}", hash, path.display()).expect("Could not write to file")
        }
        Output::Stdout(stdout) => {
            writeln!(stdout, "{},{}", hash, path.display()).expect("Could not write to stdout")
        }
    }
}

fn hash(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buffer = [0u8; 1024 * 8];
    let mut hash = Sha1::new();

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break Some(hash.digest().to_string()),
            Ok(n) => hash.update(&buffer[0..n]),
            Err(_) => panic!("NOOOPE"),
        }
    }
}

fn start_iter(working_dir: PathBuf, output_type: OutputType, pool: &ThreadPool) -> Receiver<Work> {
    let (tx, rx) = channel();
    let tx_send = tx.clone();

    pool.execute(move || {
        let out_handle = {
            match output_type {
                OutputType::Stdout => Handle::stdout().expect("Could not get handle to stdout"),
                OutputType::File(path) => {
                    Handle::from_path(path).expect("Could not get handle to output file")
                }
            }
        };

        let iter = walkdir::WalkDir::new(working_dir)
            .into_iter()
            .filter_map(|x| x.ok());

        for entry in iter {
            let handle = if let Ok(h) = Handle::from_path(entry.path()) {
                h
            } else {
                // Could not get handle
                continue;
            };

            // If output is being piped to a file, skip hashing it.
            if out_handle == handle {
                continue;
            }

            tx.send(Work::Directory {
                tx: tx_send.clone(),
                path: entry.path().into(),
            }).expect("Could not signal pool");
        }
    });

    rx
}
