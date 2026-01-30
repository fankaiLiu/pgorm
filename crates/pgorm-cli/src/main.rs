#[tokio::main]
async fn main() {
    if let Err(e) = pgorm_cli::run(std::env::args().collect()).await {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

