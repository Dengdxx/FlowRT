cmake_minimum_required(VERSION 3.20)

project(flowrt_cross_sdk_deps LANGUAGES NONE)

include(ExternalProject)

# 该 CMake 项目是公开交叉 SDK 的显式 prepare 步骤。FlowRT build 阶段只消费
# 生成的 pkg-config / include / lib overlay，不在用户构建路径里隐式联网拉取依赖。
set(FLOWRT_CROSS_SDK_PREFIX
    "${CMAKE_BINARY_DIR}/public-arm64-sdk"
    CACHE PATH "安装公开 arm64 SDK overlay 的目录")
set(FLOWRT_CROSS_CC
    "aarch64-linux-gnu-gcc"
    CACHE STRING "linux-arm64 C 交叉编译器")
set(FLOWRT_CROSS_CXX
    "aarch64-linux-gnu-g++"
    CACHE STRING "linux-arm64 C++ 交叉编译器")
set(FLOWRT_CROSS_BUILD_JOBS
    "1"
    CACHE STRING "第三方依赖构建并行度，默认保守以适配内存较小的开发机")

file(MAKE_DIRECTORY "${CMAKE_BINARY_DIR}/cmake")
set(flowrt_cross_toolchain "${CMAKE_BINARY_DIR}/cmake/linux-arm64-toolchain.cmake")
set(flowrt_cross_toolchain_in "${flowrt_cross_toolchain}.in")
file(WRITE "${flowrt_cross_toolchain_in}" [=[
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_C_COMPILER "@FLOWRT_CROSS_CC@")
set(CMAKE_CXX_COMPILER "@FLOWRT_CROSS_CXX@")
set(CMAKE_FIND_ROOT_PATH "@FLOWRT_CROSS_SDK_PREFIX@")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY BOTH)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE BOTH)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE BOTH)
]=])
configure_file("${flowrt_cross_toolchain_in}" "${flowrt_cross_toolchain}" @ONLY)

set(flowrt_pkgconfig_dir
    "${FLOWRT_CROSS_SDK_PREFIX}/lib/aarch64-linux-gnu/pkgconfig")
file(MAKE_DIRECTORY "${flowrt_pkgconfig_dir}")

ExternalProject_Add(libjpeg_turbo_arm64
    GIT_REPOSITORY https://github.com/libjpeg-turbo/libjpeg-turbo.git
    GIT_TAG 3.1.4.1
    GIT_SHALLOW TRUE
    UPDATE_DISCONNECTED TRUE
    CMAKE_ARGS
        -DCMAKE_BUILD_TYPE=Release
        -DCMAKE_TOOLCHAIN_FILE=${flowrt_cross_toolchain}
        -DCMAKE_INSTALL_PREFIX=${FLOWRT_CROSS_SDK_PREFIX}
        -DCMAKE_INSTALL_LIBDIR=lib/aarch64-linux-gnu
        -DENABLE_SHARED=OFF
        -DENABLE_STATIC=ON
        -DWITH_SIMD=OFF
        -DWITH_TURBOJPEG=OFF
    BUILD_COMMAND ${CMAKE_COMMAND} --build <BINARY_DIR> --parallel ${FLOWRT_CROSS_BUILD_JOBS}
    INSTALL_COMMAND ${CMAKE_COMMAND} --install <BINARY_DIR>)

ExternalProject_Add(kleidiai_arm64
    GIT_REPOSITORY https://github.com/ARM-software/kleidiai.git
    GIT_TAG v1.26.0-rc1
    GIT_SHALLOW TRUE
    UPDATE_DISCONNECTED TRUE
    CMAKE_ARGS
        -DCMAKE_BUILD_TYPE=Release
        -DCMAKE_TOOLCHAIN_FILE=${flowrt_cross_toolchain}
        -DCMAKE_INSTALL_PREFIX=${FLOWRT_CROSS_SDK_PREFIX}
        -DCMAKE_INSTALL_LIBDIR=lib/aarch64-linux-gnu
        -DKLEIDIAI_BUILD_TESTS=OFF
        -DKLEIDIAI_BUILD_BENCHMARK=OFF
    BUILD_COMMAND ${CMAKE_COMMAND} --build <BINARY_DIR> --parallel ${FLOWRT_CROSS_BUILD_JOBS}
    INSTALL_COMMAND ${CMAKE_COMMAND} --install <BINARY_DIR>)

set(flowrt_write_kleidiai_pc "${CMAKE_BINARY_DIR}/cmake/write-kleidiai-pc.cmake")
file(WRITE "${flowrt_write_kleidiai_pc}" [=[
file(MAKE_DIRECTORY "${pkgconfig_dir}")
file(WRITE "${pkgconfig_dir}/kleidiai.pc" "prefix=${prefix}
includedir=\${prefix}/include
libdir=\${prefix}/lib/aarch64-linux-gnu

Name: kleidiai
Description: Arm KleidiAI cross-compiled SDK overlay for FlowRT examples
Version: 1.26.0-rc1
Cflags: -I\${includedir}
Libs: -L\${libdir} -lkleidiai
")
]=])

ExternalProject_Add_Step(kleidiai_arm64 write_pkg_config
    COMMAND ${CMAKE_COMMAND}
        -Dpkgconfig_dir=${flowrt_pkgconfig_dir}
        -Dprefix=${FLOWRT_CROSS_SDK_PREFIX}
        -P ${flowrt_write_kleidiai_pc}
    DEPENDEES install
    ALWAYS TRUE)

add_custom_target(flowrt_public_arm64_sdk
    DEPENDS libjpeg_turbo_arm64 kleidiai_arm64)
