extern crate same_file;
extern crate sha1;
extern crate threadpool;
extern crate walkdir;

use same_file::Handle;
use sha1::Sha1;
use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
};
use threadpool::ThreadPool;

enum Work {
    Directory { tx: Sender<Work>, path: PathBuf },
    Hashed { hash: String, path: PathBuf },
}

fn main() {
    let stdout = io::stdout();
    let mut stream = stdout.lock();
    let pool = threadpool::Builder::new().build();
    let rx = start_iter(&pool);

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
            Work::Hashed { hash, path } => {
                writeln!(stream, "{},{}", hash, path.display()).expect("Could not write to stdout")
            }
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

fn start_iter(pool: &ThreadPool) -> Receiver<Work> {
    let working_dir = env::current_dir().expect("Could not get working directory");
    let (tx, rx) = channel();
    let tx_send = tx.clone();

    pool.execute(move || {
        let stdout_handle = Handle::stdout().expect("Could not get handle to stdout");
        let iter = walkdir::WalkDir::new(working_dir)
            .into_iter()
            .filter_map(|x| x.ok());

        for entry in iter {
            let handle = Handle::from_path(entry.path()).expect("Could not get handle to path");

            // If output is being piped to a file, skip hashing it.
            if stdout_handle == handle {
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
