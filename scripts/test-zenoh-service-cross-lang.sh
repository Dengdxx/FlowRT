#!/usr/bin/env bash
# 跨语言跨进程 zenoh service smoke 测试。
#
# 前置条件：
#   - cargo test -p flowrt --features zenoh 已通过
#   - cmake --build build/cpp-zenoh-service 已完成
#
# 注意：跨语言测试要求 Rust zenoh crate 与 C++ zenoh-c 版本 wire protocol 兼容。
# 当前 Rust 使用 zenoh 1.9.0，C++ 使用 zenoh-c 1.6.2（ROS jazzy vendor），
# 两者 wire protocol 不兼容。跨语言测试需要安装匹配版本的 zenoh-c。
#
# 用法：
#   bash scripts/test-zenoh-service-cross-lang.sh
#
# 测试内容：
#   1. C++ server + C++ client（跨进程）
#   2. Rust server + Rust client（跨进程）
#   3. Rust server + C++ client（跨语言，需版本匹配，失败不阻塞）
#   4. C++ server + Rust client（跨语言，需版本匹配，失败不阻塞）

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CPP_BUILD="$REPO_DIR/build/cpp-zenoh-service"
TIMEOUT=15

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

TEST_PORT=18800

pass() { echo -e "${GREEN}PASS${NC}: $1"; }
fail() { echo -e "${RED}FAIL${NC}: $1"; exit 1; }
warn() { echo -e "${YELLOW}WARN${NC}: $1"; }

echo "=== Building Rust cross-lang examples ==="
cd "$REPO_DIR"
cargo build --features zenoh --example zenoh_service_server --example zenoh_service_client 2>&1 | tail -3

RUST_SERVER_BIN="$REPO_DIR/target/debug/examples/zenoh_service_server"
RUST_CLIENT_BIN="$REPO_DIR/target/debug/examples/zenoh_service_client"
CPP_SERVER_BIN="$CPP_BUILD/flowrt_zenoh_service_server"
CPP_CLIENT_BIN="$CPP_BUILD/flowrt_zenoh_service_client"

for bin in "$RUST_SERVER_BIN" "$RUST_CLIENT_BIN" "$CPP_SERVER_BIN" "$CPP_CLIENT_BIN"; do
    if [[ ! -x "$bin" ]]; then
        fail "Binary not found: $bin"
    fi
done

run_cross_process_test() {
    local label="$1"
    local server_bin="$2"
    local client_bin="$3"
    local service_name="flowrt/smoke/${label}_$$"
    local zenoh_endpoint="tcp/127.0.0.1:$TEST_PORT"
    TEST_PORT=$((TEST_PORT + 1))

    export FLOWRT_ZENOH_SERVICE_NAME="$service_name"
    export FLOWRT_ZENOH_MODE="peer"
    export FLOWRT_ZENOH_NO_MULTICAST="1"

    # Server listens
    export FLOWRT_ZENOH_LISTEN="$zenoh_endpoint"
    unset FLOWRT_ZENOH_CONNECT
    "$server_bin" &
    local server_pid=$!
    sleep 3

    # Client connects (no listen)
    unset FLOWRT_ZENOH_LISTEN
    export FLOWRT_ZENOH_CONNECT="$zenoh_endpoint"
    if timeout "$TIMEOUT" "$client_bin" 2>&1; then
        pass "$label"
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
        unset FLOWRT_ZENOH_CONNECT
        return 0
    else
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
        unset FLOWRT_ZENOH_CONNECT
        return 1
    fi
}

echo ""
echo "=== Test 1: C++ server + C++ client (cross-process) ==="
run_cross_process_test "cpp_cpp" "$CPP_SERVER_BIN" "$CPP_CLIENT_BIN" || fail "C++ cross-process"

echo ""
echo "=== Test 2: Rust server + Rust client (cross-process) ==="
run_cross_process_test "rust_rust" "$RUST_SERVER_BIN" "$RUST_CLIENT_BIN" || fail "Rust cross-process"

echo ""
echo "=== Test 3: Rust server + C++ client (cross-lang) ==="
if run_cross_process_test "rust_cpp" "$RUST_SERVER_BIN" "$CPP_CLIENT_BIN"; then
    pass "Rust server + C++ client"
else
    warn "Rust server + C++ client (zenoh wire protocol version mismatch — expected if zenoh-rust != zenoh-c version)"
fi

echo ""
echo "=== Test 4: C++ server + Rust client (cross-lang) ==="
if run_cross_process_test "cpp_rust" "$CPP_SERVER_BIN" "$RUST_CLIENT_BIN"; then
    pass "C++ server + Rust client"
else
    warn "C++ server + Rust client (zenoh wire protocol version mismatch — expected if zenoh-rust != zenoh-c version)"
fi

echo ""
echo "=== All same-language cross-process tests PASSED ==="
echo "=== Cross-language tests require matching zenoh versions (Rust 1.9.0 vs C++ 1.6.2) ==="
