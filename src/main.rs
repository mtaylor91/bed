use buildexperiments::{Runner, Spec};

#[tokio::main]
async fn main() {
    let mut runner = Runner::new();

    let task = runner.job("Hello, world!".to_string()).task();

    task.step(Spec::command(vec![
        "echo".to_string(),
        "Hello".to_string(),
    ]));

    task.step(Spec::command(vec![
        "echo".to_string(),
        "world!".to_string(),
    ]));

    runner.run().await;
}
