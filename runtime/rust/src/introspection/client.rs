use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use super::model::{IntrospectionRequest, IntrospectionResponse};

/// 向 introspection socket 请求 status。
pub fn request_status(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::Status)
}

/// 向 introspection socket 请求 status，并限制 socket 读写等待时间。
pub fn request_status_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::Status, timeout)
}

/// 向 introspection socket 请求 channel snapshot。
pub fn request_channel_snapshot(
    path: &Path,
    channel: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::ChannelSnapshot {
            channel: channel.into(),
        },
    )
}

/// 向 introspection socket 请求 channel snapshot，并限制 socket 读写等待时间。
pub fn request_channel_snapshot_with_timeout(
    path: &Path,
    channel: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::ChannelSnapshot {
            channel: channel.into(),
        },
        timeout,
    )
}

/// 向 introspection socket 请求 self-description JSON。
pub fn request_self_description(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::SelfDescription)
}

/// 向 introspection socket 请求 self-description JSON，并限制 socket 读写等待时间。
pub fn request_self_description_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::SelfDescription, timeout)
}

/// 向 introspection socket 打开 observe channel 连接。
pub fn observe_channel_stream(
    path: &Path,
    channel: impl Into<String>,
) -> std::io::Result<(UnixStream, IntrospectionResponse)> {
    let mut stream = UnixStream::connect(path)?;
    let request = serde_json::to_string(&IntrospectionRequest::ObserveChannel {
        channel: channel.into(),
        mode: Some("latest".to_string()),
    })
    .map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if line.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "FlowRT observe channel response was empty",
        ));
    }
    let response = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
    Ok((stream, response))
}

/// 向 introspection socket 打开 observe channel 连接，并限制握手读写等待时间。
pub fn observe_channel_stream_with_timeout(
    path: &Path,
    channel: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<(UnixStream, IntrospectionResponse)> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let request = serde_json::to_string(&IntrospectionRequest::ObserveChannel {
        channel: channel.into(),
        mode: Some("latest".to_string()),
    })
    .map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if line.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "FlowRT observe channel response was empty",
        ));
    }
    let response = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    Ok((stream, response))
}

/// 向 introspection socket 请求参数列表。
pub fn request_param_list(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::ParamList)
}

/// 向 introspection socket 请求参数列表，并限制 socket 读写等待时间。
pub fn request_param_list_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::ParamList, timeout)
}

/// 向 introspection socket 请求单个参数状态。
pub fn request_param_get(
    path: &Path,
    name: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::ParamGet { name: name.into() })
}

/// 向 introspection socket 请求单个参数状态，并限制 socket 读写等待时间。
pub fn request_param_get_with_timeout(
    path: &Path,
    name: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::ParamGet { name: name.into() },
        timeout,
    )
}

/// 向 introspection socket 写入参数 pending 值。
pub fn request_param_set(
    path: &Path,
    name: impl Into<String>,
    value: serde_json::Value,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::ParamSet {
            name: name.into(),
            value,
        },
    )
}

/// 向 introspection socket 写入参数 pending 值，并限制 socket 读写等待时间。
pub fn request_param_set_with_timeout(
    path: &Path,
    name: impl Into<String>,
    value: serde_json::Value,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::ParamSet {
            name: name.into(),
            value,
        },
        timeout,
    )
}

/// 向 introspection socket 请求注入 island boundary input。
pub fn request_boundary_publish(
    path: &Path,
    endpoint: impl Into<String>,
    payload: Vec<u8>,
    published_at_ms: Option<u64>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::BoundaryPublish {
            endpoint: endpoint.into(),
            payload,
            published_at_ms,
        },
    )
}

/// 向 introspection socket 请求注入 island boundary input，并限制 socket 等待时间。
pub fn request_boundary_publish_with_timeout(
    path: &Path,
    endpoint: impl Into<String>,
    payload: Vec<u8>,
    published_at_ms: Option<u64>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::BoundaryPublish {
            endpoint: endpoint.into(),
            payload,
            published_at_ms,
        },
        timeout,
    )
}

/// 向 introspection socket 请求取消 operation invocation。
pub fn request_operation_cancel(
    path: &Path,
    operation_id: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::OperationCancel {
            operation_id: operation_id.into(),
        },
    )
}

/// 向 introspection socket 请求取消 operation invocation，并限制 socket 读写等待时间。
pub fn request_operation_cancel_with_timeout(
    path: &Path,
    operation_id: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::OperationCancel {
            operation_id: operation_id.into(),
        },
        timeout,
    )
}

/// 向 introspection socket 请求启动 operation endpoint。
pub fn request_operation_start(
    path: &Path,
    operation: impl Into<String>,
    payload: Vec<u8>,
    timeout_ms: Option<u64>,
    owner: Option<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::OperationStart {
            operation: operation.into(),
            payload,
            timeout_ms,
            owner,
        },
    )
}

/// 向 introspection socket 请求启动 operation endpoint，并限制 socket 读写等待时间。
pub fn request_operation_start_with_timeout(
    path: &Path,
    operation: impl Into<String>,
    payload: Vec<u8>,
    timeout_ms: Option<u64>,
    owner: Option<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::OperationStart {
            operation: operation.into(),
            payload,
            timeout_ms,
            owner,
        },
        timeout,
    )
}

/// 向 introspection socket 请求启动 recorder。
pub fn request_recorder_start(
    path: &Path,
    output: Option<String>,
    filters: Vec<String>,
    queue_depth: Option<usize>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::RecorderStart {
            output,
            filters,
            queue_depth,
        },
    )
}

/// 向 introspection socket 请求启动 recorder，并限制 socket 读写等待时间。
pub fn request_recorder_start_with_timeout(
    path: &Path,
    output: Option<String>,
    filters: Vec<String>,
    queue_depth: Option<usize>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::RecorderStart {
            output,
            filters,
            queue_depth,
        },
        timeout,
    )
}

/// 向 introspection socket 请求停止 recorder。
pub fn request_recorder_stop(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::RecorderStop)
}

/// 向 introspection socket 请求停止 recorder，并限制 socket 读写等待时间。
pub fn request_recorder_stop_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::RecorderStop, timeout)
}

/// 向 introspection socket 请求取走 recorder 暂存事件。
pub fn request_recorder_drain(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::RecorderDrain)
}

/// 向 introspection socket 请求取走 recorder 暂存事件，并限制 socket 读写等待时间。
pub fn request_recorder_drain_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::RecorderDrain, timeout)
}

fn request(path: &Path, request: &IntrospectionRequest) -> std::io::Result<IntrospectionResponse> {
    let mut stream = UnixStream::connect(path)?;
    send_request_and_read_response(&mut stream, request)
}

fn request_with_timeout(
    path: &Path,
    request: &IntrospectionRequest,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    send_request_and_read_response(&mut stream, request)
}

fn send_request_and_read_response(
    stream: &mut UnixStream,
    request: &IntrospectionRequest,
) -> std::io::Result<IntrospectionResponse> {
    let request = serde_json::to_string(request).map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}
