use bed::Loader;

#[tokio::main]
async fn main() -> Result<(), bed::Error> {
    let mut loader = Loader::new("examples".to_string());
    loader.load()?;
    loader.runner().run().await?;
    Ok(())
}
