use serde::Serialize;
use std::marker::PhantomData;
use tokio_modbus::{client::Context, prelude::Reader};

fn to_be_bytes(data: Vec<u16>) -> Vec<u8> {
    data.iter()
        .flat_map(|v| v.to_be_bytes())
        .collect::<Vec<u8>>()
}

#[derive(Debug, Serialize)]
pub(crate) struct SiteMeters {
    pub(crate) consumption_kw: f64,
    pub(crate) net_import_kw: f64,
    pub(crate) production_kw: f64,
    pub(crate) exported_kwh: f64,
    pub(crate) imported_kwh: f64,
}

pub(crate) struct ModbusRegister<K: DecodableRegister<K>> {
    pub(crate) address: u16,
    pub(crate) length: u16,
    pub(crate) key_type: PhantomData<K>,
}

impl<K: DecodableRegister<K>> ModbusRegister<K> {
    pub(crate) fn new(address: u16, length: u16) -> Self {
        Self {
            address: address,
            length: length,
            key_type: PhantomData,
        }
    }
}

pub(crate) trait DecodableRegister<T> {
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

pub(crate) async fn read_register<T: DecodableRegister<T>>(
    ctx: &mut Context,
    register: ModbusRegister<T>,
) -> T {
    let data = ctx
        .read_holding_registers(register.address, register.length)
        .await
        .unwrap();
    T::decode(data)
}
