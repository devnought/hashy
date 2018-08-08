use clap::{App, Arg};
use std::path::PathBuf;

const OUTPUT: &str = "output";

#[derive(Debug)]
pub struct CommandArg {
    pub output: Option<PathBuf>,
}

fn build_cli<'a, 'b>() -> App<'a, 'b> {
    App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::with_name(OUTPUT)
                .help("Output file path")
                .short("o")
                .long(OUTPUT)
                .takes_value(true)
                .value_name("OUTPUT"),
        )
}

pub fn print_help() {
    build_cli()
        .print_help()
        .expect("Could not print command line help message");
}

pub fn handle_args() -> Option<CommandArg> {
    let matches = build_cli().get_matches();

    let output = if let Some(o) = matches.value_of(OUTPUT) {
        Some(PathBuf::from(o))
    } else {
        None
    };

    Some(CommandArg { output })
}
