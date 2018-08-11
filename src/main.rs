#[macro_use]
extern crate clap;
extern crate content_inspector;
extern crate pbr;
extern crate sha1;
extern crate threadpool;
extern crate walkdir;

mod cli;
mod progress;

use content_inspector::ContentType;
use progress::Progress;
use sha1::Sha1;
use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Read, StdoutLock, Write},
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
    Hashed {
        hash: (String, Option<ContentType>),
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

#[derive(Clone, Debug)]
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

    let working_dir = env::current_dir().expect("Could not get working directory");
    let pool = threadpool::Builder::new().build();
    let mut pb = Progress::new(&pool);

    let (mut output, output_type) = if let Some(out) = args.output {
        pb.set_enabled(true);
        (
            Output::File(BufWriter::new(
                File::create(&out).expect("Could not create output file"),
            )),
            OutputType::File(
                out.canonicalize()
                    .expect("Could not canonicalize output path"),
            ),
        )
    } else {
        (
            Output::Stdout {
                stream: stdout.lock(),
                working_dir: working_dir
                    .canonicalize()
                    .expect("Could not get absolute working dir"),
            },
            OutputType::Stdout,
        )
    };

    pb.build_status();

    let rx = start_iter(working_dir, &pool);

    while let Ok(result) = rx.recv() {
        match result {
            Work::Directory { tx, path, index } => {
                let output_type = output_type.clone();
                pool.execute(move || {
                    let hash = match hash(&path, output_type) {
                        Some(h) => h,
                        None => {
                            tx.send(Work::Empty { index })
                                .expect("Could not signal pool");
                            return;
                        }
                    };

                    tx.send(Work::Hashed { path, hash, index })
                        .expect("Could not signal pool");
                });
            }
            Work::Empty { index: _ } => pb.inc(),
            Work::Hashed {
                hash,
                path,
                index: _,
            } => {
                let (h, c) = hash;
                print_hash(&mut output, &h, c, &path);
                pb.inc_success();
            }
            Work::DiscoveryComplete { count } => pb.build_bar(count),
        }
    }
}

fn print_hash(output: &mut Output, hash: &str, content_type: Option<ContentType>, path: &Path) {
    match output {
        Output::File(writer) => writeln!(writer, "{},{:?},{}", hash, content_type, path.display())
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

            writeln!(stream, "{} {:?} {}", hash, content_type, diff.display())
                .expect("Could not write to stdout")
        }
    }
}

fn hash(path: &Path, output_type: OutputType) -> Option<(String, Option<ContentType>)> {
    match output_type {
        OutputType::Stdout => {}
        OutputType::File(p) => if p == path.canonicalize().ok()? {
            return None;
        },
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(false)
        .create(false)
        .open(path)
        .ok()?;

    let mut buffer = [0u8; 1024 * 1024];
    let mut hash = Sha1::new();
    let mut content_type = None;

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break Some((hash.digest().to_string(), content_type)),
            Ok(n) => {
                if let None = content_type {
                    let len = if n > 1024 { 1024 } else { n };

                    content_type = Some(content_inspector::inspect(&buffer[0..len]));
                }

                hash.update(&buffer[0..n]);
            }
            Err(_) => break None,
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
