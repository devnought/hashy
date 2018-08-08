#[macro_use]
extern crate clap;
extern crate pbr;
extern crate same_file;
extern crate sha1;
extern crate threadpool;
extern crate walkdir;

use pbr::ProgressBar;
use same_file::Handle;
use sha1::Sha1;
use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Read, StdoutLock, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
};
use threadpool::ThreadPool;

mod cli;

enum Work {
    Directory {
        tx: Sender<Work>,
        path: PathBuf,
        index: usize,
    },
    Hashed {
        hash: String,
        path: PathBuf,
        index: usize,
    },
    DiscoveryComplete {
        count: usize,
    },
}

enum Output<'a> {
    File(BufWriter<File>),
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

    let (mut output, output_type, mut pb) = if let Some(out) = args.output {
        (
            Output::File(BufWriter::new(
                File::create(&out).expect("Could not create output file"),
            )),
            OutputType::File(out),
            Some({
                let mut p = ProgressBar::on(io::stderr(), 0);
                p.show_speed = false;
                p.show_percent = false;
                p.show_counter = false;
                p.show_time_left = false;
                p.message(" Processing files ");
                p
            }),
        )
    } else {
        (Output::Stdout(stdout.lock()), OutputType::Stdout, None)
    };

    let pool = threadpool::Builder::new().build();
    let working_dir = env::current_dir().expect("Could not get working directory");
    let mut progress_max = None;
    let mut progress = 0;
    let rx = start_iter(working_dir, &output_type, &pool);

    while let Ok(result) = rx.recv() {
        match result {
            Work::Directory { tx, path, index } => {
                pool.execute(move || {
                    let hash = match hash(&path) {
                        Some(h) => h,
                        None => return,
                    };

                    tx.send(Work::Hashed { path, hash, index })
                        .expect("Could not signal pool");
                });
            }
            Work::Hashed {
                hash,
                path,
                index: _,
            } => {
                progress += 1;
                print_hash(&mut output, &hash, &path);

                if let Some(p) = pb.as_mut() {
                    if let None = progress_max {
                        p.message(&format!(" Processing files ({}) ", progress));
                        p.tick();
                    } else {
                        p.inc();
                    }
                }
            }
            Work::DiscoveryComplete { count } => match &output_type {
                OutputType::File(_) => {
                    let mut p = ProgressBar::on(io::stderr(), count as u64);
                    p.set(progress);
                    p.show_speed = false;
                    p.show_time_left = false;

                    pb = Some(p);
                    progress_max = Some(count);
                }
                _ => {}
            },
        }
    }
}

fn print_hash(output: &mut Output, hash: &str, path: &Path) {
    match output {
        Output::File(writer) => {
            writeln!(writer, "{},{}", hash, path.display()).expect("Could not write to file")
        }
        Output::Stdout(stdout) => {
            writeln!(stdout, "{} {}", hash, path.display()).expect("Could not write to stdout")
        }
    }
}

fn hash(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut buffer = [0u8; 1024 * 1024];
    let mut hash = Sha1::new();

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break Some(hash.digest().to_string()),
            Ok(n) => hash.update(&buffer[0..n]),
            Err(_) => panic!("NOOOPE"),
        }
    }
}

fn start_iter(working_dir: PathBuf, output_type: &OutputType, pool: &ThreadPool) -> Receiver<Work> {
    let (tx, rx) = channel();
    let tx_send = tx.clone();

    let out_handle = {
        match output_type {
            OutputType::Stdout => Handle::stdout().expect("Could not get handle to stdout"),
            OutputType::File(path) => {
                Handle::from_path(path).expect("Could not get handle to output file")
            }
        }
    };

    pool.execute(move || {
        let iter = walkdir::WalkDir::new(working_dir)
            .into_iter()
            .filter_map(|x| x.ok());

        let mut file_count = 0;

        for entry in iter {
            let file = if let Ok(f) = OpenOptions::new()
                .read(true)
                .write(false)
                .create(false)
                .open(entry.path())
            {
                f
            } else {
                continue;
            };

            let handle = if let Ok(h) = Handle::from_file(file) {
                h
            } else {
                continue;
            };

            // If output is being piped to a file, skip hashing it.
            if out_handle == handle {
                continue;
            }

            tx.send(Work::Directory {
                tx: tx_send.clone(),
                path: entry.path().into(),
                index: file_count,
            }).expect("Could not signal pool");

            file_count += 1;
        }

        tx.send(Work::DiscoveryComplete { count: file_count })
            .expect("Could not signal pool");
    });

    rx
}
