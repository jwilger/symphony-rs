#[tokio::main]
async fn main() {
    if let Err(error) = symphony_app::run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
