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

multiarch="$(dpkg-architecture -a"$architecture" -qDEB_HOST_MULTIARCH)"

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
cp -a "$repo_root/runtime/rust/Cargo.toml" "$repo_root/runtime/rust/src" \
    "$private_root/share/flowrt/runtime/rust/"
install -d "$private_root/share/cargo"
vendor_log="$package_work/cargo-vendor.log"
if ! cargo vendor --locked --versioned-dirs "$private_root/share/cargo/vendor" \
    >"$vendor_log" 2>&1; then
    cat "$vendor_log" >&2
    exit 1
fi
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

for name in iceoryx2 zenoh-c zenoh-cpp; do
    require_git_lock "$name"
    fetch_git_snapshot "$name" "${lock_git_url[$name]}" "${lock_git_tag[$name]}"
done

third_party_doc="$private_root/share/doc/flowrt/third-party"
install -d "$third_party_doc"
cp "$vendor_src_dir/iceoryx2/LICENSE-MIT" "$third_party_doc/iceoryx2.LICENSE"
cp "$vendor_src_dir/zenoh-c/LICENSE" "$third_party_doc/zenoh-c.LICENSE"
cp "$vendor_src_dir/zenoh-cpp/LICENSE" "$third_party_doc/zenoh-cpp.LICENSE"

iox2_build="$package_work/iceoryx2"
cmake -S "$vendor_src_dir/iceoryx2" -B "$iox2_build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$private_prefix" \
    -DCMAKE_INSTALL_LIBDIR="lib/${multiarch}" \
    -DBUILD_CXX=ON \
    -DBUILD_EXAMPLES=OFF \
    -DBUILD_TESTING=OFF
cmake --build "$iox2_build"
DESTDIR="$staging" cmake --install "$iox2_build"

zenoh_root="$package_work/zenoh-root"
mkdir -p "$zenoh_root"
for deb_name in "libzenohc_1.9.0_${architecture}.deb" "libzenohc-dev_1.9.0_${architecture}.deb" libzenohcpp-dev_1.9.0_all.deb; do
    require_deb_lock "$deb_name"
    dpkg-deb -x "$(download_cached "${lock_deb_url[$deb_name]}")" "$zenoh_root"
done
if [[ -d "$zenoh_root/usr/include" ]]; then
    install -d "$private_root/include"
    cp -a "$zenoh_root/usr/include/." "$private_root/include/"
fi
if [[ -d "$zenoh_root/usr/lib" ]]; then
    install -d "$private_root/lib"
    cp -a "$zenoh_root/usr/lib/." "$private_root/lib/"
fi

cmake_build="$package_work/cpp-runtime"
cmake -S "$repo_root/runtime/cpp" -B "$cmake_build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$private_prefix" \
    -DCMAKE_INSTALL_LIBDIR="lib/${multiarch}"
cmake --build "$cmake_build"
DESTDIR="$staging" cmake --install "$cmake_build"

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
