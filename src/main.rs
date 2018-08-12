#[macro_use]
extern crate clap;
extern crate content_inspector;
extern crate pbr;
extern crate sha1;
#[macro_use]
extern crate tantivy;
extern crate threadpool;
extern crate walkdir;

mod cli;
mod processor;
mod progress;

use processor::ParsedFile;
use progress::Progress;
use std::{
    env,
    fs::File,
    io::{self, BufWriter, StdoutLock, Write},
    path::{Path, PathBuf},
    sync::mpsc::{channel, Receiver, Sender},
};
use tantivy::{
    collector::TopCollector,
    directory::Directory,
    query::QueryParser,
    schema::{SchemaBuilder, STORED, TEXT},
    Document, Index,
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

    let mut schema_builder = SchemaBuilder::default();
    schema_builder.add_text_field("title", TEXT | STORED);
    schema_builder.add_text_field("body", TEXT);

    let schema = schema_builder.build();
    let index_path = Path::new("/temp/tantivy");
    let index =
        Index::create_in_dir(index_path, schema.clone()).expect("Could not create tantivy index");
    let mut index_writer = index
        .writer(50_000_000)
        .expect("Could not create index writer");

    let title = schema.get_field("title").unwrap();
    let body = schema.get_field("body").unwrap();

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
                print_hash(&mut output, &entry, &path);
                let mut doc = Document::default();
                doc.add_text(title, &format!("{}", path.display()));

                if let Some(content) = entry.str_content() {
                    doc.add_text(body, content);
                }

                index_writer.add_document(doc);

                pb.inc_success();
            }
            Work::DiscoveryComplete { count } => pb.build_bar(count),
        }
    }

    index_writer.commit().expect("Could not commit tantivy");
    index.load_searchers().expect("Could not load searchers");

    let searcher = index.searcher();
    let mut query_parser = QueryParser::for_index(&index, vec![title, body]);
    let query = query_parser
        .parse_query("this is let mut")
        .expect("Could nto parse query");
    let mut top_collector = TopCollector::with_limit(10);

    searcher
        .search(&*query, &mut top_collector)
        .expect("Could not search");
    let doc_addresss = top_collector.docs();

    for doc in doc_addresss {
        let d = searcher.doc(&doc);

        match output {
            Output::File(ref mut writer) => writeln!(writer, "{:?}", d).expect("Could not write doc"),
            Output::Stdout { ref mut stream, .. } => writeln!(stream, "{:?}", d).expect("Couyld not wrute doc"),
        }
    }
}

fn print_hash(output: &mut Output, entry: &ParsedFile, path: &Path) {
    let content_type = entry
        .content_type()
        .map(|x| format!("{:?}", x))
        .unwrap_or_else(|| String::from("EMPTY"));

    match output {
        Output::File(writer) => writeln!(
            writer,
            "{},{},{}",
            entry.hash(),
            content_type,
            path.display()
        ).expect("Could not write to file"),
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

            writeln!(
                stream,
                "{} {} {}",
                entry.hash(),
                content_type,
                diff.display()
            ).expect("Could not write to stdout");
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
