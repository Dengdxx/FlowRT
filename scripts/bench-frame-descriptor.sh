#!/usr/bin/env bash
set -euo pipefail

iters="${FLOWRT_BENCH_ITERS:-20000}"
payload_bytes="${FLOWRT_BENCH_PAYLOAD_BYTES:-6220800}"

case "$iters" in
    '' | *[!0-9]*)
        printf 'FLOWRT_BENCH_ITERS must be a positive integer\n' >&2
        exit 2
        ;;
esac

case "$payload_bytes" in
    '' | *[!0-9]*)
        printf 'FLOWRT_BENCH_PAYLOAD_BYTES must be a positive integer\n' >&2
        exit 2
        ;;
esac

if [[ "$iters" -eq 0 || "$payload_bytes" -eq 0 ]]; then
    printf 'FLOWRT_BENCH_ITERS and FLOWRT_BENCH_PAYLOAD_BYTES must be greater than zero\n' >&2
    exit 2
fi

command -v rustc >/dev/null || {
    printf 'rustc is required for frame descriptor microbench\n' >&2
    exit 1
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flowrt-frame-bench.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

source_file="$work_dir/frame_descriptor_bench.rs"
binary_file="$work_dir/frame_descriptor_bench"

cat >"$source_file" <<'RS'
use std::hint::black_box;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FrameDescriptorFields {
    resource_id_hash: u64,
    slot: u32,
    generation: u64,
    size_bytes: u64,
    timestamp_unix_ns: u64,
    width: u32,
    height: u32,
    stride_bytes: u32,
    format_id: u32,
    encoding_id: u32,
    flags: u32,
}

fn encode(fields: FrameDescriptorFields) -> [u8; 64] {
    let mut out = [0u8; 64];
    out[0..8].copy_from_slice(&fields.resource_id_hash.to_le_bytes());
    out[8..12].copy_from_slice(&fields.slot.to_le_bytes());
    out[16..24].copy_from_slice(&fields.generation.to_le_bytes());
    out[24..32].copy_from_slice(&fields.size_bytes.to_le_bytes());
    out[32..40].copy_from_slice(&fields.timestamp_unix_ns.to_le_bytes());
    out[40..44].copy_from_slice(&fields.width.to_le_bytes());
    out[44..48].copy_from_slice(&fields.height.to_le_bytes());
    out[48..52].copy_from_slice(&fields.stride_bytes.to_le_bytes());
    out[52..56].copy_from_slice(&fields.format_id.to_le_bytes());
    out[56..60].copy_from_slice(&fields.encoding_id.to_le_bytes());
    out[60..64].copy_from_slice(&fields.flags.to_le_bytes());
    out
}

fn decode(bytes: [u8; 64]) -> FrameDescriptorFields {
    FrameDescriptorFields {
        resource_id_hash: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        slot: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        generation: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        size_bytes: u64::from_le_bytes(bytes[24..32].try_into().unwrap()),
        timestamp_unix_ns: u64::from_le_bytes(bytes[32..40].try_into().unwrap()),
        width: u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
        height: u32::from_le_bytes(bytes[44..48].try_into().unwrap()),
        stride_bytes: u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
        format_id: u32::from_le_bytes(bytes[52..56].try_into().unwrap()),
        encoding_id: u32::from_le_bytes(bytes[56..60].try_into().unwrap()),
        flags: u32::from_le_bytes(bytes[60..64].try_into().unwrap()),
    }
}

fn percentile(samples: &mut [u128], numerator: usize, denominator: usize) -> u128 {
    samples.sort_unstable();
    let last = samples.len().saturating_sub(1);
    let index = (last * numerator) / denominator;
    samples[index]
}

fn summarize(label: &str, samples: &[u128], unit: &str) {
    let mut p50 = samples.to_vec();
    let mut p95 = samples.to_vec();
    let mut p99 = samples.to_vec();
    println!(
        "{label} p50={}{} p95={}{} p99={}{}",
        percentile(&mut p50, 50, 100),
        unit,
        percentile(&mut p95, 95, 100),
        unit,
        percentile(&mut p99, 99, 100),
        unit,
    );
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let iters: usize = args[1].parse().unwrap();
    let payload_bytes: usize = args[2].parse().unwrap();

    let mut descriptor_samples = Vec::with_capacity(iters);
    let mut descriptor = FrameDescriptorFields {
        resource_id_hash: 0xF081,
        slot: 7,
        generation: 0,
        size_bytes: payload_bytes as u64,
        timestamp_unix_ns: 0,
        width: 1920,
        height: 1080,
        stride_bytes: 5760,
        format_id: 1,
        encoding_id: 1,
        flags: 0,
    };
    let mut checksum = 0u64;
    for i in 0..iters {
        descriptor.generation = i as u64;
        descriptor.timestamp_unix_ns = (i as u64) * 20_000_000;
        let start = Instant::now();
        let bytes = encode(black_box(descriptor));
        let decoded = decode(black_box(bytes));
        checksum ^= decoded.generation ^ decoded.size_bytes;
        descriptor_samples.push(start.elapsed().as_nanos());
    }

    let src = (0..payload_bytes)
        .map(|index| (index as u8).wrapping_mul(31))
        .collect::<Vec<_>>();
    let mut dst = vec![0u8; payload_bytes];
    let mut memcpy_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        dst.copy_from_slice(black_box(&src));
        checksum ^= dst[payload_bytes / 2] as u64;
        memcpy_samples.push(start.elapsed().as_nanos());
    }

    println!("frame_descriptor_microbench iters={iters} payload_bytes={payload_bytes}");
    summarize("descriptor_roundtrip", &descriptor_samples, "ns");
    summarize("payload_memcpy", &memcpy_samples, "ns");
    println!("checksum={}", black_box(checksum));
}
RS

rustc --edition=2024 -O "$source_file" -o "$binary_file"
"$binary_file" "$iters" "$payload_bytes"
