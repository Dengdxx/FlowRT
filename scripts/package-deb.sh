#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/package-deb.sh [--output-dir DIR] [--version VERSION] [--architecture ARCH]

Build a single FlowRT Debian package containing the flowrt CLI, Rust runtime
crate, C++ runtime headers, and CMake package files.
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

if [[ -z "$version" ]]; then
    version="$(
        awk '
            $1 == "version" && $2 == "=" {
                gsub(/"/, "", $3);
                print $3;
                exit;
            }
        ' "$repo_root/Cargo.toml"
    )"
fi
if [[ -z "$version" ]]; then
    printf 'failed to read FlowRT version from Cargo.toml\n' >&2
    exit 1
fi

if [[ -z "$architecture" ]]; then
    architecture="$(dpkg --print-architecture)"
fi
multiarch="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"

package_work_parent="$repo_root/build/package-deb"
mkdir -p "$package_work_parent" "$output_dir"
package_work="$(mktemp -d "$package_work_parent/work.XXXXXX")"
package_root="$package_work/flowrt_${version}_${architecture}"
staging="$package_root/root"
mkdir -p "$staging"

cargo build --release -p flowrt-cli

install -D -m 0755 "$repo_root/target/release/flowrt" "$staging/usr/bin/flowrt"

install -d "$staging/usr/share/flowrt/runtime/rust"
cp -a "$repo_root/runtime/rust/Cargo.toml" "$repo_root/runtime/rust/src" \
    "$staging/usr/share/flowrt/runtime/rust/"

cmake_build="$package_work/cpp-runtime"
cmake -S "$repo_root/runtime/cpp" -B "$cmake_build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX=/usr \
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
installed_size="$(du -sk "$staging/usr" | awk '{print $1}')"
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
 development files so user projects can build generated applications without
 cloning the FlowRT source repository.
EOF

find "$staging" -type d -exec chmod 0755 {} +
find "$staging" -type f -name '*.cmake' -exec chmod 0644 {} +
chmod 0644 "$staging/usr/share/doc/flowrt/changelog.gz" \
    "$staging/usr/share/doc/flowrt/copyright" \
    "$control_dir/control"

package_path="$output_dir/flowrt_${version}_${architecture}.deb"
dpkg-deb --build --root-owner-group "$staging" "$package_path"
printf '%s\n' "$package_path"
