use std::{marker::PhantomData, net::SocketAddr};

use clap::Parser;
use tokio::time::{sleep, Duration};
use tokio_modbus::{
    client::{tcp, Context},
    prelude::Reader,
    slave::Slave,
};

use serde::Serialize;

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
struct SiteMeters {
    consumption_kw: f64,
    net_import_kw: f64,
    production_kw: f64,
    exported_kwh: f64,
    imported_kwh: f64,
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

fn to_be_bytes(data: Vec<u16>) -> Vec<u8> {
    data.iter()
        .flat_map(|v| v.to_be_bytes())
        .collect::<Vec<u8>>()
}

struct ModbusRegister<K: DecodableRegister<K>> {
    address: u16,
    length: u16,
    key_type: PhantomData<K>,
}

impl<K: DecodableRegister<K>> ModbusRegister<K> {
    fn new(address: u16, length: u16) -> Self {
        Self {
            address: address,
            length: length,
            key_type: PhantomData,
        }
    }
}

trait DecodableRegister<T> {
    fn decode(data: Vec<u16>) -> T;
}

impl DecodableRegister<String> for String {
    fn decode(data: Vec<u16>) -> String {
        let data: Vec<u8> = to_be_bytes(data)
            .iter()
            .filter(|b| **b != 0)
            .map(|b| *b)
            .collect();
        String::from_utf8(data).expect("Converting data to string")
    }
}

impl DecodableRegister<i16> for i16 {
    fn decode(data: Vec<u16>) -> i16 {
        data[0] as i16
    }
}

impl DecodableRegister<u32> for u32 {
    fn decode(data: Vec<u16>) -> u32 {
        u32::from_be_bytes(<[u8; 4]>::try_from(to_be_bytes(data)).unwrap())
    }
}

async fn read_register<T: DecodableRegister<T>>(
    ctx: &mut Context,
    register: ModbusRegister<T>,
) -> T {
    let data = ctx
        .read_holding_registers(register.address, register.length)
        .await
        .unwrap();
    T::decode(data)
}

async fn send_pv_data(args: &Args) -> anyhow::Result<()> {
    // let host = "192.168.40.10:1502".parse().unwrap();
    let mut ctx = tcp::connect_slave(args.host_address, Slave(1)).await?;

    // I_AC_Power
    let point: ModbusRegister<i16> = ModbusRegister::new(40083, 1);
    let ac_power = read_register(&mut ctx, point).await;

    // I_AC_Power_SF
    let point: ModbusRegister<i16> = ModbusRegister::new(40084, 1);
    let ac_power_scale = read_register(&mut ctx, point).await;

    // Convert to W with scale factor
    let current_ac_power: f64 = ac_power as f64 * 10f64.powi(ac_power_scale.into());

    // Convert to kW
    let production_kw = current_ac_power / 1000f64;

    // M_Exported
    let point: ModbusRegister<u32> = ModbusRegister::new(40226, 2);
    let exported_energy = read_register(&mut ctx, point).await;

    // M_Imported
    let point: ModbusRegister<u32> = ModbusRegister::new(40234, 2);
    let imported_energy = read_register(&mut ctx, point).await;

    // M_Energy_W_SF
    let point: ModbusRegister<i16> = ModbusRegister::new(40242, 1);
    let energy_scale = read_register(&mut ctx, point).await;

    // Convert to Wh with scale factor
    let exported_energy_wh = exported_energy as f64 * 10f64.powi(energy_scale.into());
    let imported_energy_wh = imported_energy as f64 * 10f64.powi(energy_scale.into());

    // Convert to kWh
    let exported_energy_kwh = exported_energy_wh / 1000f64;
    let imported_energy_kwh = imported_energy_wh / 1000f64;

    // M_AC_Power
    let point: ModbusRegister<i16> = ModbusRegister::new(40206, 1);
    let meter_power = read_register(&mut ctx, point).await;

    // M_AC_Power_SF
    let point: ModbusRegister<i16> = ModbusRegister::new(40210, 1);
    let meter_power_sf = read_register(&mut ctx, point).await;

    let meter_power_w = meter_power as f64 * 10f64.powi(meter_power_sf.into());
    let meter_power_kw = -meter_power_w / 1000f64;

    // Consumption (I_AC_Power + M_AC_POWER)
    let total_consumption = production_kw + meter_power_kw;

    let meters = SiteMeters {
        consumption_kw: total_consumption,
        net_import_kw: meter_power_kw,
        production_kw: production_kw,
        imported_kwh: imported_energy_kwh,
        exported_kwh: exported_energy_kwh,
    };
    let payload = ChargeHqPayload::new(&args.api_key, meters);
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    loop {
        match send_pv_data(&args).await {
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
