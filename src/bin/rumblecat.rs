use dotenv::dotenv;
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let host = env::var("RUMBLECAT_HOST").unwrap();
    let port: u16 = str::parse(&env::var("RUMBLECAT_PORT").unwrap()).unwrap();

    let connector = rumblecat::MumbleConnector::new(host, port);
    rumblecat::serve(connector).await;
    std::thread::park()
}
