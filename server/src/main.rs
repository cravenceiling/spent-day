use clap::Parser;

/// Self-hostable Spent Day sync server. Stores encrypted batches it cannot read.
#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:8787")]
    listen: String,
    #[arg(long, default_value = "./spentday-server.db")]
    db: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let conn = spentday_server::open_db(&args.db)?;
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    eprintln!(
        "spentday-server listening on http://{} (db {})",
        listener.local_addr()?,
        args.db
    );
    spentday_server::serve(listener, conn).await?;
    Ok(())
}
