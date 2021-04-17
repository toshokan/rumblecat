use dotenv::dotenv;
use std::env;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let host = env::var("RUMBLECAT_HOST").unwrap();
    let port: u16 = str::parse(&env::var("RUMBLECAT_PORT").unwrap()).unwrap();
    
    struct NoOp;
    impl rumblecat::MessageHandler for NoOp {}
    
    rumblecat::connect(&host, port, NoOp{}).await.unwrap();
}
