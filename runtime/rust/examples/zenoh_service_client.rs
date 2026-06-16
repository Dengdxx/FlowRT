//! 或编译为独立 binary 运行。

#![cfg(feature = "zenoh")]

use flowrt::{
    ServiceResult, WireCodec, WireCodecError,
    zenoh::{ZenohServiceClient, config_from_environment},
};
use zenoh::Wait;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddRequest {
    pub a: i32,
    pub b: i32,
}

impl WireCodec for AddRequest {
    const WIRE_SIZE: usize = 8;
    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        output[..4].copy_from_slice(&self.a.to_le_bytes());
        output[4..].copy_from_slice(&self.b.to_le_bytes());
        Ok(())
    }
    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            a: i32::from_le_bytes([input[0], input[1], input[2], input[3]]),
            b: i32::from_le_bytes([input[4], input[5], input[6], input[7]]),
        })
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddResponse {
    pub sum: i32,
}

impl WireCodec for AddResponse {
    const WIRE_SIZE: usize = 4;
    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        output.copy_from_slice(&self.sum.to_le_bytes());
        Ok(())
    }
    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            sum: i32::from_le_bytes([input[0], input[1], input[2], input[3]]),
        })
    }
}

pub fn service_name() -> String {
    std::env::var("FLOWRT_ZENOH_SERVICE_NAME")
        .unwrap_or_else(|_| "flowrt/cross_lang/add".to_string())
}

pub fn open_session() -> zenoh::Session {
    zenoh::open(config_from_environment().unwrap_or_default())
        .wait()
        .expect("zenoh session should open")
}

fn main() {
    let session = open_session();
    let name = service_name();

    let client = ZenohServiceClient::<AddRequest, AddResponse>::open(&name, session.clone());

    eprintln!("[rust-client] calling service '{name}'...");

    let result = client.call(AddRequest { a: 10, b: 20 }, 5000);
    match result {
        ServiceResult::Ok(resp) => {
            eprintln!("[rust-client] OK: 10 + 20 = {}", resp.sum);
            assert_eq!(resp.sum, 30, "expected 30, got {}", resp.sum);
            eprintln!("[rust-client] PASS");
        }
        ServiceResult::Err(code, msg) => {
            eprintln!("[rust-client] FAIL: code={code:?} msg={msg:?}");
            std::process::exit(1);
        }
    }
}
