use axum::{extract::Path, routing::get, Json, Router};
use bed::{Loader, JobTracker};
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
    let tracker = JobTracker::new();
    let tracker_clone = tracker.clone();

    let build_future = tokio::spawn(async move {
        loader.load()?;
        loader.runner().run(tracker_clone).await?;
        Ok::<(), bed::Error>(())
    });

    let get_job = |name: Path<String>| async move {
        let job = tracker.get(&name);
        Json(job)
    };

    let app = Router::new()
        .route("/job/:name", get(get_job));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;

    axum::serve(listener, app).await?;

    build_future.await??;

    Ok(())
}
