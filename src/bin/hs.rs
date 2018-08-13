extern crate clap;
extern crate tantivy;

use clap::{App, Arg};
use std::path::Path;
use tantivy::{
    collector::TopCollector,
    query::QueryParser,
    schema::{SchemaBuilder, STORED, TEXT},
    Index,
};

fn build_cli<'a, 'b>() -> App<'a, 'b> {
    App::new("search").arg(
        Arg::with_name("query")
            .help("Query")
            .index(1)
            .takes_value(true)
            .required(true)
            .value_name("query"),
    )
}

pub fn handle_args() -> Option<String> {
    let matches = build_cli().get_matches();

    matches.value_of("query").map(|x| Some(String::from(x)))?
}

fn main() {
    let query_args = handle_args().expect("Invalid args");

    let index_path = Path::new("/temp/tantivy");
    let index = Index::open_in_dir(index_path).expect("Could not open index");

    let mut schema_builder = SchemaBuilder::default();
    schema_builder.add_text_field("title", TEXT | STORED);
    schema_builder.add_text_field("body", TEXT);

    let schema = schema_builder.build();

    let title = schema.get_field("title").unwrap();
    let body = schema.get_field("body").unwrap();

    let searcher = index.searcher();
    let query_parser = QueryParser::for_index(&index, vec![title, body]);
    let query = query_parser
        .parse_query(&query_args)
        .expect("Could nto parse query");
    let mut top_collector = TopCollector::with_limit(10);

    searcher
        .search(&*query, &mut top_collector)
        .expect("Could not search");
    let doc_addresss = top_collector.docs();

    for doc in doc_addresss {
        let d = searcher.doc(&doc).expect("Could not retrieve doc");
        println!("{}", schema.to_json(&d));
    }
}
