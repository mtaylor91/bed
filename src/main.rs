use bed::Loader;
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[clap(short, long, default_value = ".bed")]
    directory: String,
}

#[tokio::main]
async fn main() -> Result<(), bed::Error> {
    let args = Args::parse();
    let mut loader = Loader::new(args.directory);
    loader.load()?;
    loader.runner().run().await?;
    Ok(())
}
