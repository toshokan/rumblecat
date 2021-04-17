use dotenv::dotenv;
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let host = env::var("RUMBLECAT_HOST").unwrap();
    let port: u16 = str::parse(&env::var("RUMBLECAT_PORT").unwrap()).unwrap();

    let connector = rumblecat::MumbleConnector::new(host, port);
    // let mut connection = connector.connect("rumblecat", &host, port).await.unwrap();
    // while let Some(msg) = connection.rx.recv().await {
    // 	dbg!(msg);
    // }

    rumblecat::serve(connector).await;
    std::thread::park()
}
