//! Zenoh service request/response 集成测试。

#![cfg(feature = "zenoh")]

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use flowrt::{
    ServiceError, ServiceResult, WireCodec, WireCodecError,
    zenoh::{ZenohServiceClient, ZenohServiceServer},
};
use zenoh::{Config, Wait};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddRequest {
    a: i32,
    b: i32,
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
struct AddResponse {
    sum: i32,
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

fn unique_service_name(suffix: &str) -> String {
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    format!(
        "flowrt_test_{}_{}_{}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed),
        suffix
    )
}

fn open_session() -> zenoh::Session {
    zenoh::open(Config::default())
        .wait()
        .expect("zenoh session should open")
}

#[test]
fn zenoh_service_basic_request_response() {
    let session = open_session();
    let service_name = unique_service_name("basic");

    let _server = ZenohServiceServer::<AddRequest, AddResponse>::open(
        &service_name,
        session.clone(),
        |req: AddRequest| ServiceResult::ok(AddResponse { sum: req.a + req.b }),
    )
    .expect("server should open");

    let client =
        ZenohServiceClient::<AddRequest, AddResponse>::open(&service_name, session.clone());

    let result = client.call(AddRequest { a: 3, b: 4 }, 5000);
    if result.is_err() {
        eprintln!(
            "call failed: code={:?} msg={:?}",
            result.error_code(),
            result.error_message()
        );
    }
    assert!(result.is_ok());
    assert_eq!(result.ok_value().unwrap().sum, 7);
}

#[test]
fn zenoh_service_handler_error() {
    let session = open_session();
    let service_name = unique_service_name("handler_error");

    let _server = ZenohServiceServer::<AddRequest, AddResponse>::open(
        &service_name,
        session.clone(),
        |_req: AddRequest| {
            ServiceResult::err_with_message(ServiceError::HandlerError, "division by zero")
        },
    )
    .expect("server should open");

    let client =
        ZenohServiceClient::<AddRequest, AddResponse>::open(&service_name, session.clone());

    let result = client.call(AddRequest { a: 1, b: 2 }, 5000);
    if result.is_err() {
        eprintln!(
            "call failed: code={:?} msg={:?}",
            result.error_code(),
            result.error_message()
        );
    }
    assert!(result.is_err());
    assert_eq!(result.error_code(), ServiceError::HandlerError);
    assert_eq!(result.error_message(), Some("division by zero"));
}

#[test]
fn zenoh_service_timeout() {
    let session = open_session();
    let service_name = unique_service_name("timeout");
    let handler_done = Arc::new(AtomicBool::new(false));

    let handler_done_clone = Arc::clone(&handler_done);
    let _server = ZenohServiceServer::<AddRequest, AddResponse>::open(
        &service_name,
        session.clone(),
        move |_req: AddRequest| {
            thread::sleep(Duration::from_millis(200));
            handler_done_clone.store(true, Ordering::Release);
            ServiceResult::ok(AddResponse { sum: 0 })
        },
    )
    .expect("server should open");

    let client =
        ZenohServiceClient::<AddRequest, AddResponse>::open(&service_name, session.clone());

    let result = client.call(AddRequest { a: 1, b: 2 }, 50);
    eprintln!(
        "timeout test: code={:?} msg={:?}",
        result.error_code(),
        result.error_message()
    );
    assert!(result.is_err());
    assert_eq!(result.error_code(), ServiceError::Timeout);

    for _ in 0..20 {
        if handler_done.load(Ordering::Acquire) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timeout test handler did not finish before teardown");
}

#[test]
fn zenoh_service_unavailable() {
    let session = open_session();
    let service_name = unique_service_name("unavailable");

    // 不创建 server，只创建 client
    let client =
        ZenohServiceClient::<AddRequest, AddResponse>::open(&service_name, session.clone());

    // 调用一个没有 server 的 service，应该超时
    let result = client.call(AddRequest { a: 1, b: 2 }, 500);
    assert!(result.is_err());
    assert_eq!(result.error_code(), ServiceError::Timeout);
}

#[test]
fn zenoh_service_multiple_clients() {
    let session = open_session();
    let service_name = unique_service_name("multi_client");

    let _server = ZenohServiceServer::<AddRequest, AddResponse>::open(
        &service_name,
        session.clone(),
        |req: AddRequest| ServiceResult::ok(AddResponse { sum: req.a + req.b }),
    )
    .expect("server should open");

    let mut handles = Vec::new();
    for i in 0..3 {
        let service_name = service_name.clone();
        let session = session.clone();
        handles.push(thread::spawn(move || {
            let client =
                ZenohServiceClient::<AddRequest, AddResponse>::open(&service_name, session);

            let result = client.call(AddRequest { a: i, b: i * 2 }, 5000);
            if result.is_err() {
                eprintln!(
                    "client {} call failed: code={:?} msg={:?}",
                    i,
                    result.error_code(),
                    result.error_message()
                );
            }
            assert!(result.is_ok());
            assert_eq!(result.ok_value().unwrap().sum, i + i * 2);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
