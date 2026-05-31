use clap::Parser;

#[derive(clap::Args, Debug)]
struct IngestArgs {
    #[arg(long)]
    strict: bool,
}

#[derive(clap::Parser, Debug)]
struct RunArgs {
    #[arg(long)]
    strict: bool,
    #[command(flatten)]
    ingest: IngestArgs,
}

fn main() {
    RunArgs::parse_from(["test"]);
}
