#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/package-deb.sh [--output-dir DIR] [--version VERSION] [--architecture ARCH]

Build a single FlowRT Debian package containing the flowrt CLI, Rust runtime
crate, C++ runtime headers, CMake package files, vendored Rust crates, and
the locked C++ backend SDKs used by generated applications.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$repo_root/dist"
version=""
architecture=""

while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --output-dir)
            output_dir="${2:?missing value for --output-dir}"
            shift 2
            ;;
        --version)
            version="${2:?missing value for --version}"
            shift 2
            ;;
        --architecture)
            architecture="${2:?missing value for --architecture}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

command -v cargo >/dev/null || {
    printf 'cargo is required to build flowrt\n' >&2
    exit 1
}
command -v cmake >/dev/null || {
    printf 'cmake is required to package the C++ runtime\n' >&2
    exit 1
}
command -v dpkg-deb >/dev/null || {
    printf 'dpkg-deb is required to build a Debian package\n' >&2
    exit 1
}
command -v dpkg-architecture >/dev/null || {
    printf 'dpkg-architecture is required to resolve Debian architecture paths\n' >&2
    exit 1
}
command -v curl >/dev/null || {
    printf 'curl is required to fetch vendored C++ backend SDK packages\n' >&2
    exit 1
}
command -v git >/dev/null || {
    printf 'git is required to fetch vendored C++ backend source snapshots\n' >&2
    exit 1
}
command -v sha256sum >/dev/null || {
    printf 'sha256sum is required to verify vendored Debian packages\n' >&2
    exit 1
}

# ---------------------------------------------------------------------------
# 依赖锁定校验
# ---------------------------------------------------------------------------
deps_lock="$repo_root/scripts/deps.lock"
if [[ ! -f "$deps_lock" ]]; then
    printf 'dependency lock file not found: %s\n' "$deps_lock" >&2
    exit 1
fi

declare -A lock_git_commit   # name -> expected commit
declare -A lock_git_url      # name -> url
declare -A lock_git_tag      # name -> tag
declare -A lock_deb_sha256   # basename -> expected sha256
declare -A lock_deb_url      # basename -> url

while IFS=' ' read -r type name version_val url checksum; do
    [[ -z "$type" || "$type" == \#* ]] && continue
    case "$type" in
        git)
            lock_git_commit["$name"]="$checksum"
            lock_git_url["$name"]="$url"
            lock_git_tag["$name"]="$version_val"
            ;;
        deb)
            lock_deb_sha256["$name"]="$checksum"
            lock_deb_url["$name"]="$url"
            ;;
        *)
            printf 'deps.lock: unknown type %s\n' "$type" >&2
            exit 1
            ;;
    esac
done < "$deps_lock"

require_git_lock() {
    local name="$1"
    if [[ -z "${lock_git_url[$name]:-}" || -z "${lock_git_tag[$name]:-}" || -z "${lock_git_commit[$name]:-}" ]]; then
        printf 'deps.lock: missing git lock entry for %s\n' "$name" >&2
        exit 1
    fi
}

require_deb_lock() {
    local name="$1"
    if [[ -z "${lock_deb_url[$name]:-}" || -z "${lock_deb_sha256[$name]:-}" ]]; then
        printf 'deps.lock: missing deb lock entry for %s\n' "$name" >&2
        exit 1
    fi
}

if [[ -z "$version" ]]; then
    version="$(
        awk '
            $1 == "version" && $2 == "=" {
                gsub(/"/, "", $3);
                print $3;
                exit;
            }
        ' "$repo_root/Cargo.toml"
    )" || true
fi
if [[ -z "$version" ]]; then
    printf '错误: 无法从 %s 读取 workspace version。\n' "$repo_root/Cargo.toml" >&2
    printf '请确认 Cargo.toml 中 [workspace.package] 的 version 字段存在且格式正确，\n' >&2
    printf '或使用 --version 参数显式指定版本。\n' >&2
    exit 1
fi

if [[ -z "$architecture" ]]; then
    architecture="$(dpkg --print-architecture)"
fi
case "$architecture" in
    amd64|arm64)
        ;;
    *)
        printf '错误: --architecture %s 暂不支持。\n' "$architecture" >&2
        printf '当前支持架构: amd64 arm64\n' >&2
        exit 1
        ;;
esac

flowrt_platform_for_architecture() {
    case "$1" in
        amd64)
            printf 'linux-amd64\n'
            ;;
        arm64)
            printf 'linux-arm64\n'
            ;;
        *)
            printf 'unsupported Debian architecture for FlowRT target SDK: %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

multiarch_for_architecture() {
    dpkg-architecture -a"$1" -qDEB_HOST_MULTIARCH
}

rust_target_for_architecture() {
    case "$1" in
        amd64)
            printf 'x86_64-unknown-linux-gnu\n'
            ;;
        arm64)
            printf 'aarch64-unknown-linux-gnu\n'
            ;;
        *)
            printf 'unsupported Debian architecture for Rust target: %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

c_compiler_for_architecture() {
    case "$1" in
        amd64)
            printf 'gcc\n'
            ;;
        arm64)
            printf 'aarch64-linux-gnu-gcc\n'
            ;;
        *)
            printf 'unsupported Debian architecture for C compiler: %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

cpp_compiler_for_architecture() {
    case "$1" in
        amd64)
            printf 'g++\n'
            ;;
        arm64)
            printf 'aarch64-linux-gnu-g++\n'
            ;;
        *)
            printf 'unsupported Debian architecture for C++ compiler: %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

cmake_processor_for_architecture() {
    case "$1" in
        amd64)
            printf 'x86_64\n'
            ;;
        arm64)
            printf 'aarch64\n'
            ;;
        *)
            printf 'unsupported Debian architecture for CMake processor: %s\n' "$1" >&2
            exit 1
            ;;
    esac
}

require_cross_toolchain() {
    local target_architecture="$1"
    local rust_target="$(rust_target_for_architecture "$target_architecture")"
    local c_compiler="$(c_compiler_for_architecture "$target_architecture")"
    local cpp_compiler="$(cpp_compiler_for_architecture "$target_architecture")"

    command -v "$c_compiler" >/dev/null || {
        printf 'missing C cross compiler for %s: %s\n' "$target_architecture" "$c_compiler" >&2
        printf 'install gcc-aarch64-linux-gnu before building the amd64 package with linux-arm64 SDK\n' >&2
        exit 1
    }
    command -v "$cpp_compiler" >/dev/null || {
        printf 'missing C++ cross compiler for %s: %s\n' "$target_architecture" "$cpp_compiler" >&2
        printf 'install g++-aarch64-linux-gnu before building the amd64 package with linux-arm64 SDK\n' >&2
        exit 1
    }
    if command -v rustup >/dev/null; then
        if ! rustup target list --installed | grep -Fxq "$rust_target"; then
            printf 'missing Rust target for %s: %s\n' "$target_architecture" "$rust_target" >&2
            printf 'run: rustup target add %s\n' "$rust_target" >&2
            exit 1
        fi
    fi
}

host_architecture="$(dpkg --print-architecture)"
if [[ "$architecture" != "$host_architecture" ]]; then
    printf '错误: 当前 package-deb.sh 只支持原生架构打包。\n' >&2
    printf '请求架构: %s\n当前主机架构: %s\n' "$architecture" "$host_architecture" >&2
    printf '请在匹配架构的 runner/机器上运行，或等交叉编译打包支持落地后再使用。\n' >&2
    exit 1
fi

platform="$(flowrt_platform_for_architecture "$architecture")"
multiarch="$(multiarch_for_architecture "$architecture")"

package_work_parent="$repo_root/build/package-deb"
mkdir -p "$package_work_parent" "$output_dir"
package_work="$(mktemp -d "$package_work_parent/work.XXXXXX")"
package_root="$package_work/flowrt_${version}_${architecture}"
staging="$package_root/root"
mkdir -p "$staging"
private_prefix="/opt/flowrt/${version}"
private_root="$staging${private_prefix}"
cache_dir="$package_work_parent/cache"
vendor_src_dir="$package_work_parent/vendor-src"
mkdir -p "$cache_dir" "$vendor_src_dir"

cargo build --release -p flowrt-cli

install -D -m 0755 "$repo_root/target/release/flowrt" "$private_root/bin/flowrt"
install -d "$staging/usr/bin"
ln -s "${private_prefix}/bin/flowrt" "$staging/usr/bin/flowrt"

install -d "$private_root/share/flowrt/runtime/rust"
cp -a "$repo_root/runtime/rust/Cargo.toml" "$repo_root/runtime/rust/examples" "$repo_root/runtime/rust/src" \
    "$private_root/share/flowrt/runtime/rust/"
install -d "$private_root/share/flowrt/crates/flowrt-record"
cp -a "$repo_root/crates/flowrt-record/src" "$private_root/share/flowrt/crates/flowrt-record/"
cat > "$private_root/share/flowrt/crates/flowrt-record/Cargo.toml" <<EOF
[package]
name = "flowrt-record"
version = "${version}"
edition = "2024"
rust-version = "1.85"
license = "MIT"

[dependencies]
mcap = { version = "0.24.0", default-features = false }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
EOF
install -d "$private_root/share/cargo"
vendor_log="$package_work/cargo-vendor.log"
if ! cargo vendor --locked --versioned-dirs "$private_root/share/cargo/vendor" \
    >"$vendor_log" 2>&1; then
    cat "$vendor_log" >&2
    exit 1
fi
vendor_hash="$(
    for relative in Cargo.lock runtime/rust/Cargo.toml crates/flowrt-record/Cargo.toml scripts/deps.lock; do
        path="$repo_root/$relative"
        if [[ -f "$path" ]]; then
            printf '%s' "$relative"
            cat "$path"
        fi
    done | sha256sum | awk '{print substr($1, 1, 16)}'
)"
printf '%s  -\n' "$vendor_hash" > "$private_root/share/cargo/vendor/.flowrt-vendor.sha256"
cat > "$private_root/share/cargo/config.toml" <<EOF
[source.crates-io]
replace-with = "flowrt-vendor"

[source.flowrt-vendor]
directory = "${private_prefix}/share/cargo/vendor"

[net]
offline = true
EOF

fetch_git_snapshot() {
    local name="$1"
    local repo="$2"
    local tag="$3"
    local expected_commit="${lock_git_commit[$name]:-}"
    local dest="$vendor_src_dir/$name"
    if [[ -d "$dest/.git" ]]; then
        git -C "$dest" fetch --depth 1 origin "refs/tags/${tag}:refs/tags/${tag}" >/dev/null
        git -C "$dest" checkout --detach "$tag" >/dev/null
    else
        rm -rf "$dest"
        git clone --depth 1 --branch "$tag" "$repo" "$dest"
    fi
    local actual_commit
    actual_commit="$(git -C "$dest" rev-parse HEAD)"
    if [[ "$actual_commit" != "$expected_commit" ]]; then
        printf 'FATAL: %s tag %s commit mismatch\n  expected: %s\n  actual:   %s\n' \
            "$name" "$tag" "$expected_commit" "$actual_commit" >&2
        exit 1
    fi
}

download_cached() {
    local url="$1"
    local basename="$(basename "$url")"
    local dest="$cache_dir/$basename"
    local expected_sha256="${lock_deb_sha256[$basename]:-}"
    if [[ -z "$expected_sha256" ]]; then
        printf 'FATAL: %s not found in deps.lock\n' "$basename" >&2
        exit 1
    fi
    if [[ ! -f "$dest" ]]; then
        curl -fsSL "$url" -o "$dest"
    fi
    local actual_sha256
    actual_sha256="$(sha256sum "$dest" | awk '{print $1}')"
    if [[ "$actual_sha256" != "$expected_sha256" ]]; then
        printf 'FATAL: %s sha256 mismatch\n  expected: %s\n  actual:   %s\n' \
            "$basename" "$expected_sha256" "$actual_sha256" >&2
        rm -f "$dest"
        exit 1
    fi
    printf '%s\n' "$dest"
}

copy_required_tree() {
    local source="$1"
    local dest="$2"
    local label="$3"
    if [[ ! -d "$source" ]]; then
        printf 'FATAL: missing %s directory: %s\n' "$label" "$source" >&2
        exit 1
    fi
    install -d "$dest"
    cp -a "$source/." "$dest/"
}

copy_optional_tree() {
    local source="$1"
    local dest="$2"
    if [[ -d "$source" ]]; then
        install -d "$dest"
        cp -a "$source/." "$dest/"
    fi
}

copy_lib_root_files() {
    local source="$1"
    local dest="$2"
    local copied=0
    install -d "$dest"
    local file
    while IFS= read -r file; do
        cp -a "$file" "$dest/"
        copied=1
    done < <(find "$source" -maxdepth 1 -type f | sort)
    if [[ "$copied" -eq 0 ]]; then
        printf 'FATAL: missing target SDK root libraries in %s\n' "$source" >&2
        exit 1
    fi
}

require_packaged_file() {
    local path="$1"
    local label="$2"
    if [[ ! -f "$path" ]]; then
        printf 'FATAL: missing %s file: %s\n' "$label" "$path" >&2
        exit 1
    fi
}

write_cmake_package_wrapper_from_source() {
    local package_name="$1"
    local target_relative_config="$2"
    local target_relative_version="$3"
    local wrapper_dir="$4"
    install -d "$wrapper_dir"
    cat > "$wrapper_dir/${package_name}Config.cmake" <<EOF
include("\${CMAKE_CURRENT_LIST_DIR}/${target_relative_config}")
EOF
    if [[ -n "$target_relative_version" ]]; then
        cat > "$wrapper_dir/${package_name}ConfigVersion.cmake" <<EOF
include("\${CMAKE_CURRENT_LIST_DIR}/${target_relative_version}")
EOF
    fi
}

install_multiarch_cmake_wrappers() {
    local sdk_root="$1"
    local sdk_multiarch="$2"
    local source_cmake_root="$sdk_root/lib/${sdk_multiarch}/cmake"
    local package_dir
    for package_dir in "$source_cmake_root"/*; do
        [[ -d "$package_dir" ]] || continue
        local package_name
        package_name="$(basename "$package_dir")"
        if [[ -f "$package_dir/${package_name}Config.cmake" ]]; then
            local version_relative=""
            if [[ -f "$package_dir/${package_name}ConfigVersion.cmake" ]]; then
                version_relative="../../lib/${sdk_multiarch}/cmake/${package_name}/${package_name}ConfigVersion.cmake"
            fi
            write_cmake_package_wrapper_from_source "$package_name" \
                "../../lib/${sdk_multiarch}/cmake/${package_name}/${package_name}Config.cmake" \
                "$version_relative" \
                "$sdk_root/cmake/$package_name"
        fi
    done
}

install_root_cmake_wrappers() {
    local sdk_root="$1"
    local source_cmake_root="$sdk_root/lib/cmake"
    local package_dir
    for package_dir in "$source_cmake_root"/*; do
        [[ -d "$package_dir" ]] || continue
        local package_name
        package_name="$(basename "$package_dir")"
        if [[ -f "$package_dir/${package_name}Config.cmake" ]]; then
            local version_relative=""
            if [[ -f "$package_dir/${package_name}ConfigVersion.cmake" ]]; then
                version_relative="../../lib/cmake/${package_name}/${package_name}ConfigVersion.cmake"
            fi
            write_cmake_package_wrapper_from_source "$package_name" \
                "../../lib/cmake/${package_name}/${package_name}Config.cmake" \
                "$version_relative" \
                "$sdk_root/cmake/$package_name"
        fi
    done
}

rewrite_target_pkgconfig_files() {
    local sdk_root="$1"
    local sdk_platform="$2"
    local pc_file
    while IFS= read -r pc_file; do
        sed -i "s#^prefix=.*#prefix=${private_prefix}/targets/${sdk_platform}#" "$pc_file"
    done < <(find "$sdk_root/pkgconfig" -type f -name '*.pc' | sort)
}

write_target_sdk_manifest() {
    local sdk_root="$1"
    local sdk_platform="$2"
    local sdk_architecture="$3"
    local sdk_multiarch="$4"
    local complete="$5"
    local host_mirror="$6"
    local reason="$7"
    local components="$8"
    cat > "$sdk_root/flowrt-target-sdk.toml" <<EOF
schema_version = 1
platform = "${sdk_platform}"
architecture = "${sdk_architecture}"
multiarch = "${sdk_multiarch}"
complete = ${complete}
host_mirror = ${host_mirror}
reason = "${reason}"
include_dir = "include"
lib_dir = "lib"
cmake_dir = "cmake"
pkgconfig_dir = "pkgconfig"
components = [${components}]
EOF
}

require_complete_target_sdk_files() {
    local sdk_root="$1"
    local sdk_multiarch="$2"

    require_packaged_file "$sdk_root/include/flowrt/runtime.hpp" "FlowRT C++ runtime header"
    require_packaged_file "$sdk_root/include/zenoh.h" "zenoh-c header"
    require_packaged_file "$sdk_root/include/zenoh.hxx" "zenoh-cpp header"
    require_packaged_file "$sdk_root/lib/libzenohc.so" "zenoh-c shared library"
    require_packaged_file "$sdk_root/lib/${sdk_multiarch}/libiceoryx2_cxx.a" \
        "iceoryx2-cxx static library"
    require_packaged_file "$sdk_root/lib/${sdk_multiarch}/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake" \
        "FlowRT runtime multiarch CMake config"
    require_packaged_file "$sdk_root/lib/${sdk_multiarch}/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake" \
        "iceoryx2-cxx multiarch CMake config"
    require_packaged_file "$sdk_root/cmake/flowrt_runtime/flowrt_runtimeConfig.cmake" \
        "FlowRT runtime CMake config"
    require_packaged_file "$sdk_root/cmake/iceoryx2-cxx/iceoryx2-cxxConfig.cmake" \
        "iceoryx2-cxx CMake config"
    require_packaged_file "$sdk_root/cmake/zenohc/zenohcConfig.cmake" "zenoh-c CMake config"
    require_packaged_file "$sdk_root/cmake/zenohcxx/zenohcxxConfig.cmake" \
        "zenoh-cpp CMake config"
    require_packaged_file "$sdk_root/pkgconfig/zenohc.pc" "zenoh-c pkg-config file"
    require_packaged_file "$sdk_root/pkgconfig/zenohcxx.pc" "zenoh-cpp pkg-config file"
}

install_zenoh_sdk_for_architecture() {
    local sdk_architecture="$1"
    local sdk_root="$2"
    local zenoh_root="$package_work/zenoh-root-${sdk_architecture}"

    rm -rf "$zenoh_root"
    mkdir -p "$zenoh_root"
    for deb_name in "libzenohc_1.9.0_${sdk_architecture}.deb" \
        "libzenohc-dev_1.9.0_${sdk_architecture}.deb" \
        libzenohcpp-dev_1.9.0_all.deb; do
        require_deb_lock "$deb_name"
        dpkg-deb -x "$(download_cached "${lock_deb_url[$deb_name]}")" "$zenoh_root"
    done
    if [[ -d "$zenoh_root/usr/include" ]]; then
        install -d "$sdk_root/include"
        cp -a "$zenoh_root/usr/include/." "$sdk_root/include/"
    fi
    if [[ -d "$zenoh_root/usr/lib" ]]; then
        install -d "$sdk_root/lib"
        cp -a "$zenoh_root/usr/lib/." "$sdk_root/lib/"
    fi
}

cmake_target_args_for_architecture() {
    local target_architecture="$1"
    local target_c_compiler="$(c_compiler_for_architecture "$target_architecture")"
    local target_cpp_compiler="$(cpp_compiler_for_architecture "$target_architecture")"
    local target_processor="$(cmake_processor_for_architecture "$target_architecture")"

    if [[ "$target_architecture" == "$host_architecture" ]]; then
        return 0
    fi

    printf '%s\n' \
        "-DCMAKE_SYSTEM_NAME=Linux" \
        "-DCMAKE_SYSTEM_PROCESSOR=${target_processor}" \
        "-DCMAKE_C_COMPILER=${target_c_compiler}" \
        "-DCMAKE_CXX_COMPILER=${target_cpp_compiler}"
}

build_iceoryx2_for_architecture() {
    local target_architecture="$1"
    local target_multiarch="$2"
    local install_prefix="$3"
    local build_dir="$4"
    local cmake_args=()

    mapfile -t cmake_args < <(cmake_target_args_for_architecture "$target_architecture")
    if [[ "$target_architecture" != "$host_architecture" ]]; then
        require_cross_toolchain "$target_architecture"
        cmake_args+=("-DRUST_TARGET_TRIPLET=$(rust_target_for_architecture "$target_architecture")")
    fi

    cmake -S "$vendor_src_dir/iceoryx2" -B "$build_dir" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$install_prefix" \
        -DCMAKE_INSTALL_LIBDIR="lib/${target_multiarch}" \
        -DBUILD_CXX=ON \
        -DBUILD_EXAMPLES=OFF \
        -DBUILD_TESTING=OFF \
        "${cmake_args[@]}"
    if [[ "$target_architecture" == "arm64" && "$target_architecture" != "$host_architecture" ]]; then
        CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$(c_compiler_for_architecture "$target_architecture")" \
            cmake --build "$build_dir" -j1
    else
        cmake --build "$build_dir" -j1
    fi
    DESTDIR="$staging" cmake --install "$build_dir"
}

build_cpp_runtime_for_architecture() {
    local target_architecture="$1"
    local target_multiarch="$2"
    local install_prefix="$3"
    local build_dir="$4"
    local cmake_args=()

    mapfile -t cmake_args < <(cmake_target_args_for_architecture "$target_architecture")
    cmake -S "$repo_root/runtime/cpp" -B "$build_dir" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$install_prefix" \
        -DCMAKE_INSTALL_LIBDIR="lib/${target_multiarch}" \
        "${cmake_args[@]}"
    cmake --build "$build_dir" -j1
    DESTDIR="$staging" cmake --install "$build_dir"
}

install_complete_target_sdk() {
    local sdk_platform="$1"
    local sdk_architecture="$2"
    local sdk_multiarch="$3"
    local sdk_root="$private_root/targets/$sdk_platform"
    install -d "$sdk_root/include" "$sdk_root/lib" "$sdk_root/cmake" "$sdk_root/pkgconfig"

    copy_required_tree "$private_root/include" "$sdk_root/include" "target SDK include"
    copy_lib_root_files "$private_root/lib" "$sdk_root/lib"
    copy_required_tree "$private_root/lib/${sdk_multiarch}" "$sdk_root/lib/${sdk_multiarch}" \
        "target SDK multiarch lib"
    copy_required_tree "$private_root/lib/cmake" "$sdk_root/lib/cmake" "target SDK root CMake"
    copy_optional_tree "$private_root/lib/pkgconfig" "$sdk_root/pkgconfig"
    install_multiarch_cmake_wrappers "$sdk_root" "$sdk_multiarch"
    install_root_cmake_wrappers "$sdk_root"
    rewrite_target_pkgconfig_files "$sdk_root" "$sdk_platform"
    require_complete_target_sdk_files "$sdk_root" "$sdk_multiarch"

    write_target_sdk_manifest "$sdk_root" "$sdk_platform" "$sdk_architecture" \
        "$sdk_multiarch" true true "native-package-host-mirror" \
        '"flowrt-cpp-runtime", "iceoryx2-cxx", "zenoh-c", "zenoh-cpp"'
}

install_cross_complete_target_sdk() {
    local sdk_platform="$1"
    local sdk_architecture="$2"
    local sdk_multiarch="$3"
    local sdk_root="$private_root/targets/$sdk_platform"
    local sdk_prefix="${private_prefix}/targets/${sdk_platform}"

    install -d "$sdk_root/include" "$sdk_root/lib" "$sdk_root/cmake" "$sdk_root/pkgconfig"
    install_zenoh_sdk_for_architecture "$sdk_architecture" "$sdk_root"
    build_iceoryx2_for_architecture "$sdk_architecture" "$sdk_multiarch" "$sdk_prefix" \
        "$package_work/iceoryx2-${sdk_architecture}"
    build_cpp_runtime_for_architecture "$sdk_architecture" "$sdk_multiarch" "$sdk_prefix" \
        "$package_work/cpp-runtime-${sdk_architecture}"

    copy_optional_tree "$sdk_root/lib/pkgconfig" "$sdk_root/pkgconfig"
    install_multiarch_cmake_wrappers "$sdk_root" "$sdk_multiarch"
    install_root_cmake_wrappers "$sdk_root"
    rewrite_target_pkgconfig_files "$sdk_root" "$sdk_platform"
    require_complete_target_sdk_files "$sdk_root" "$sdk_multiarch"

    write_target_sdk_manifest "$sdk_root" "$sdk_platform" "$sdk_architecture" \
        "$sdk_multiarch" true false "cross-target-sdk" \
        '"flowrt-cpp-runtime", "iceoryx2-cxx", "zenoh-c", "zenoh-cpp"'
}

install_placeholder_target_sdk() {
    local sdk_platform="$1"
    local sdk_architecture="$2"
    local sdk_multiarch="$3"
    local sdk_root="$private_root/targets/$sdk_platform"
    install -d "$sdk_root/include" "$sdk_root/lib" "$sdk_root/cmake" "$sdk_root/pkgconfig"
    write_target_sdk_manifest "$sdk_root" "$sdk_platform" "$sdk_architecture" \
        "$sdk_multiarch" false false "not-built-in-this-native-package" ''
}

install_target_sdks() {
    local native_platform="$1"
    local native_architecture="$2"
    local native_multiarch="$3"
    install_complete_target_sdk "$native_platform" "$native_architecture" "$native_multiarch"

    local candidate_arch
    for candidate_arch in amd64 arm64; do
        if [[ "$candidate_arch" == "$native_architecture" ]]; then
            continue
        fi
        local candidate_platform
        local candidate_multiarch
        candidate_platform="$(flowrt_platform_for_architecture "$candidate_arch")"
        candidate_multiarch="$(multiarch_for_architecture "$candidate_arch")"
        if [[ "$native_architecture" == "amd64" && "$candidate_arch" == "arm64" ]]; then
            install_cross_complete_target_sdk "$candidate_platform" "$candidate_arch" \
                "$candidate_multiarch"
        else
            install_placeholder_target_sdk "$candidate_platform" "$candidate_arch" \
                "$candidate_multiarch"
        fi
    done
}

for name in iceoryx2 zenoh-c zenoh-cpp; do
    require_git_lock "$name"
    fetch_git_snapshot "$name" "${lock_git_url[$name]}" "${lock_git_tag[$name]}"
done

third_party_doc="$private_root/share/doc/flowrt/third-party"
install -d "$third_party_doc"
cp "$vendor_src_dir/iceoryx2/LICENSE-MIT" "$third_party_doc/iceoryx2.LICENSE"
cp "$vendor_src_dir/zenoh-c/LICENSE" "$third_party_doc/zenoh-c.LICENSE"
cp "$vendor_src_dir/zenoh-cpp/LICENSE" "$third_party_doc/zenoh-cpp.LICENSE"

build_iceoryx2_for_architecture "$architecture" "$multiarch" "$private_prefix" \
    "$package_work/iceoryx2-${architecture}"
install_zenoh_sdk_for_architecture "$architecture" "$private_root"
build_cpp_runtime_for_architecture "$architecture" "$multiarch" "$private_prefix" \
    "$package_work/cpp-runtime-${architecture}"

install_target_sdks "$platform" "$architecture" "$multiarch"

install -d "$staging/usr/share/doc/flowrt"
gzip -9c "$repo_root/CHANGELOG.md" > "$staging/usr/share/doc/flowrt/changelog.gz"
cat > "$staging/usr/share/doc/flowrt/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: FlowRT
Source: https://github.com/Dengdxx/FlowRT

Files: *
Copyright: FlowRT contributors
License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in all
 copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 SOFTWARE.
EOF

control_dir="$staging/DEBIAN"
install -d "$control_dir"
installed_size="$(du -sk "$staging" | awk '{print $1}')"
cat > "$control_dir/control" <<EOF
Package: flowrt
Version: ${version}
Section: devel
Priority: optional
Architecture: ${architecture}
Maintainer: FlowRT maintainers <dengdx@tju.edu.cn>
Installed-Size: ${installed_size}
Depends: libc6, libgcc-s1, libstdc++6
Description: Dataflow-compiled robotics runtime toolchain
 FlowRT installs the flowrt CLI together with matching Rust and C++ runtime
 development files, vendored Rust crates, and locked C++ backend SDKs so user
 projects can build generated applications without cloning the FlowRT source
 repository or downloading backend dependencies during generated builds.
EOF

find "$staging" -type d -exec chmod 0755 {} +
find "$staging" -type f -name '*.cmake' -exec chmod 0644 {} +
find "$private_root/share/cargo" -type f -exec chmod 0644 {} +
chmod 0644 "$staging/usr/share/doc/flowrt/changelog.gz" \
    "$staging/usr/share/doc/flowrt/copyright" \
    "$control_dir/control"

package_path="$output_dir/flowrt_${version}_${architecture}.deb"
dpkg-deb --build --root-owner-group "$staging" "$package_path"
printf '%s\n' "$package_path"
