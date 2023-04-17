use std::net::SocketAddr;
use tokio_modbus::{client::tcp, slave::Slave};
pub use types::SiteMeters;
use types::{read_register, ModbusRegister};

pub mod types;

pub(crate) async fn read_modbus_data(host: SocketAddr) -> anyhow::Result<SiteMeters> {
    let mut ctx = tcp::connect_slave(host, Slave(1)).await?;

    // I_AC_Power
    let point: ModbusRegister<i16> = ModbusRegister::new(40083, 1);
    let ac_power = read_register(&mut ctx, point).await?;

    // I_AC_Power_SF
    let point: ModbusRegister<i16> = ModbusRegister::new(40084, 1);
    let ac_power_scale = read_register(&mut ctx, point).await?;

    // Convert to W with scale factor
    let current_ac_power: f64 = ac_power as f64 * 10f64.powi(ac_power_scale.into());

    // Convert to kW
    let production_kw = current_ac_power / 1000f64;

    // M_Exported
    let point: ModbusRegister<u32> = ModbusRegister::new(40226, 2);
    let exported_energy = read_register(&mut ctx, point).await?;

    // M_Imported
    let point: ModbusRegister<u32> = ModbusRegister::new(40234, 2);
    let imported_energy = read_register(&mut ctx, point).await?;

    // M_Energy_W_SF
    let point: ModbusRegister<i16> = ModbusRegister::new(40242, 1);
    let energy_scale = read_register(&mut ctx, point).await?;

    // Convert to Wh with scale factor
    let exported_energy_wh = exported_energy as f64 * 10f64.powi(energy_scale.into());
    let imported_energy_wh = imported_energy as f64 * 10f64.powi(energy_scale.into());

    // Convert to kWh
    let exported_energy_kwh = exported_energy_wh / 1000f64;
    let imported_energy_kwh = imported_energy_wh / 1000f64;

    // M_AC_Power
    let point: ModbusRegister<i16> = ModbusRegister::new(40206, 1);
    let meter_power = read_register(&mut ctx, point).await?;

    // M_AC_Power_SF
    let point: ModbusRegister<i16> = ModbusRegister::new(40210, 1);
    let meter_power_sf = read_register(&mut ctx, point).await?;

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
    Ok(meters)
}
