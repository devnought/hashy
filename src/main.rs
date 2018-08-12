#[macro_use]
extern crate clap;
extern crate content_inspector;
extern crate pbr;
extern crate sha1;
extern crate threadpool;
extern crate walkdir;

mod cli;
mod processor;
mod progress;

use content_inspector::ContentType;
use processor::ParsedFile;
use progress::Progress;
use std::{
    env,
    fs::File,
    io::{self, BufWriter, StdoutLock, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
};
use threadpool::ThreadPool;

enum Work {
    Directory {
        tx: Sender<Work>,
        path: PathBuf,
        index: usize,
    },
    Parsed {
        entry: ParsedFile,
        path: PathBuf,
        index: usize,
    },
    Empty {
        index: usize,
    },
    DiscoveryComplete {
        count: u64,
    },
}

enum Output<'a> {
    File(BufWriter<File>),
    Stdout {
        stream: StdoutLock<'a>,
        working_dir: PathBuf,
    },
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

    let working_dir = env::current_dir().expect("Could not get working directory");
    let pool = threadpool::Builder::new().build();
    let mut pb = Progress::new(&pool);

    let mut output = if let Some(out) = args.output {
        pb.set_enabled(true);
        Output::File(BufWriter::new(
            File::create(&out).expect("Could not create output file"),
        ))
    } else {
        Output::Stdout {
            stream: stdout.lock(),
            working_dir: working_dir
                .canonicalize()
                .expect("Could not get absolute working dir"),
        }
    };

    pb.build_status();

    let rx = start_iter(working_dir, &pool);

    while let Ok(result) = rx.recv() {
        match result {
            Work::Directory { tx, path, index } => {
                pool.execute(move || {
                    let entry = match processor::process(&path) {
                        Some(h) => h,
                        None => {
                            tx.send(Work::Empty { index })
                                .expect("Could not signal pool");
                            return;
                        }
                    };

                    tx.send(Work::Parsed { path, entry, index })
                        .expect("Could not signal pool");
                });
            }
            Work::Empty { index: _ } => pb.inc(),
            Work::Parsed {
                entry,
                path,
                index: _,
            } => {
                print_hash(&mut output, entry.hash(), entry.content_type(), &path);
                pb.inc_success();
            }
            Work::DiscoveryComplete { count } => pb.build_bar(count),
        }
    }
}

fn print_hash(output: &mut Output, hash: &str, content_type: Option<ContentType>, path: &Path) {
    let content_type = content_type
        .map(|x| format!("{:?}", x))
        .unwrap_or_else(|| String::from("EMPTY"));

    match output {
        Output::File(writer) => writeln!(writer, "{},{},{}", hash, content_type, path.display())
            .expect("Could not write to file"),
        Output::Stdout {
            stream,
            working_dir,
        } => {
            let absolute_path = path
                .canonicalize()
                .expect("Could not get absolute file path");
            let diff = absolute_path
                .strip_prefix(working_dir)
                .expect("Could not generate relative path");

            writeln!(stream, "{} {} {}", hash, content_type, diff.display())
                .expect("Could not write to stdout")
        }
    }
}

fn start_iter(working_dir: PathBuf, pool: &ThreadPool) -> Receiver<Work> {
    let (tx, rx) = channel();
    let tx_send = tx.clone();

    pool.execute(move || {
        let iter = walkdir::WalkDir::new(working_dir)
            .into_iter()
            .filter_map(|x| x.ok());

        let mut count = 0;

        for (index, entry) in iter.enumerate() {
            tx.send(Work::Directory {
                tx: tx_send.clone(),
                path: entry.path().into(),
                index,
            }).expect("Could not signal pool");

            count = index;
        }

        tx.send(Work::DiscoveryComplete {
            count: count as u64,
        }).expect("Could not signal pool");
    });

    rx
}
