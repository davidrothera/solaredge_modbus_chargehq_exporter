use crate::modbus::SiteMeters;
use clap::Parser;
use serde::Serialize;
use std::net::SocketAddr;
use tokio::time::{sleep, Duration};
use tokio_retry::{
    strategy::{jitter, FibonacciBackoff},
    Retry,
};

mod modbus;

#[derive(Debug, clap::Parser)]
struct Args {
    #[clap(long)]
    /// API Key for pushing data to ChargeHQ
    api_key: String,

    #[clap(long)]
    /// Address/port for connecting to SolarEdge modbus interface
    host_address: SocketAddr,

    #[clap(long)]
    #[arg(default_value_t = 35)]
    /// Duration to sleep for between iterations (seconds)
    sleep_duration_secs: u64,
}

#[derive(Debug, Serialize)]
struct ChargeHqPayload {
    #[serde(rename(serialize = "apiKey"))]
    api_key: String,

    #[serde(rename(serialize = "siteMeters"))]
    site_meters: SiteMeters,
}

impl ChargeHqPayload {
    fn new(api_key: &str, meters: SiteMeters) -> Self {
        ChargeHqPayload {
            api_key: api_key.to_owned(),
            site_meters: meters,
        }
    }
}

async fn submit_pv_data(args: &Args, pv_data: SiteMeters) -> anyhow::Result<()> {
    let payload = ChargeHqPayload::new(&args.api_key, pv_data);
    println!("{:#?}", payload);

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.chargehq.net/api/public/push-solar-data")
        .json(&payload)
        .send()
        .await?;

    match res.status() {
        reqwest::StatusCode::OK => {
            println!("Success!")
        }

        code => {
            println!("Bad response code! {}", code);
            println!("{:?}", res.bytes().await?);
        }
    }

    Ok(())
}

async fn run_loop() -> anyhow::Result<()> {
    let args = Args::parse();
    let modbus_data = modbus::read_modbus_data(args.host_address).await?;
    submit_pv_data(&args, modbus_data).await?;
    Ok(())
}

async fn retry_loop() -> anyhow::Result<()> {
    let retry_policy = FibonacciBackoff::from_millis(10).map(jitter).take(5);

    Retry::spawn(retry_policy, run_loop).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    loop {
        match retry_loop().await {
            Ok(_) => {
                println!("Data sent, sleeping {}s", args.sleep_duration_secs);
            }

            Err(error) => {
                println!("Some error happened, will retry next loop! - {}", error)
            }
        }
        sleep(Duration::from_secs(args.sleep_duration_secs)).await;
    }
}
