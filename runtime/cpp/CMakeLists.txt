cmake_minimum_required(VERSION 3.22)

project(flowrt_runtime_cpp LANGUAGES CXX)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

include(CMakePackageConfigHelpers)
include(GNUInstallDirs)

find_package(Threads REQUIRED)

add_library(flowrt_runtime INTERFACE)
add_library(flowrt::runtime ALIAS flowrt_runtime)
set_target_properties(flowrt_runtime PROPERTIES EXPORT_NAME runtime)
target_include_directories(
    flowrt_runtime
    INTERFACE
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:${CMAKE_INSTALL_INCLUDEDIR}>
)
target_compile_features(flowrt_runtime INTERFACE cxx_std_20)
target_link_libraries(flowrt_runtime INTERFACE Threads::Threads)

install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(TARGETS flowrt_runtime EXPORT flowrt_runtimeTargets)
install(
    EXPORT flowrt_runtimeTargets
    NAMESPACE flowrt::
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/flowrt_runtime
)
configure_package_config_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/cmake/flowrt_runtimeConfig.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/flowrt_runtimeConfig.cmake
    INSTALL_DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/flowrt_runtime
)
write_basic_package_version_file(
    ${CMAKE_CURRENT_BINARY_DIR}/flowrt_runtimeConfigVersion.cmake
    VERSION 0.1.0
    COMPATIBILITY SameMajorVersion
)
install(
    FILES
        ${CMAKE_CURRENT_BINARY_DIR}/flowrt_runtimeConfig.cmake
        ${CMAKE_CURRENT_BINARY_DIR}/flowrt_runtimeConfigVersion.cmake
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/flowrt_runtime
)

enable_testing()

add_executable(flowrt_runtime_smoke tests/runtime_smoke.cpp)
target_link_libraries(flowrt_runtime_smoke PRIVATE flowrt_runtime)
add_test(NAME flowrt_runtime_smoke COMMAND flowrt_runtime_smoke)

add_executable(flowrt_runtime_introspection_smoke tests/introspection_smoke.cpp)
target_link_libraries(flowrt_runtime_introspection_smoke PRIVATE flowrt_runtime)
add_test(NAME flowrt_runtime_introspection_smoke COMMAND flowrt_runtime_introspection_smoke)

option(FLOWRT_CPP_ENABLE_IOX2_TESTS "Build FlowRT C++ runtime tests that require iceoryx2-cxx" OFF)
option(FLOWRT_CPP_ENABLE_ZENOH_TESTS "Build FlowRT C++ runtime tests that require zenoh-cpp" OFF)

if(FLOWRT_CPP_ENABLE_IOX2_TESTS)
    find_package(iceoryx2-cxx 0.9.1 QUIET)
    if(NOT TARGET iceoryx2-cxx::static-lib-cxx)
        message(FATAL_ERROR "iceoryx2-cxx 0.9.1 was not found. Install the FlowRT package or set CMAKE_PREFIX_PATH to the FlowRT private prefix.")
    endif()

    add_executable(flowrt_runtime_iox2_smoke tests/iox2_smoke.cpp)
    target_link_libraries(flowrt_runtime_iox2_smoke PRIVATE flowrt_runtime iceoryx2-cxx::static-lib-cxx)
    target_compile_definitions(flowrt_runtime_iox2_smoke PRIVATE FLOWRT_HAS_ICEORYX2_CXX=1 FLOWRT_ENABLE_TEST_HOOKS=1)
    add_test(NAME flowrt_runtime_iox2_smoke COMMAND flowrt_runtime_iox2_smoke)
endif()

if(FLOWRT_CPP_ENABLE_ZENOH_TESTS)
    set(FLOWRT_ZENOH_CXX_TARGET "")
    find_package(zenohc 1.9.0 QUIET)
    find_package(zenohcxx 1.9.0 QUIET)
    if(TARGET zenohcxx::zenohc)
        set(FLOWRT_ZENOH_CXX_TARGET zenohcxx::zenohc)
    endif()
    if(NOT FLOWRT_ZENOH_CXX_TARGET)
        message(FATAL_ERROR "zenoh C++ target is unavailable. Install zenoh-c and zenoh-cpp 1.9.0 so CMake exposes zenohc::lib and zenohcxx::zenohc, then set CMAKE_PREFIX_PATH if needed.")
    endif()

    add_executable(flowrt_runtime_zenoh_smoke tests/zenoh_smoke.cpp)
    target_link_libraries(flowrt_runtime_zenoh_smoke PRIVATE flowrt_runtime ${FLOWRT_ZENOH_CXX_TARGET})
    target_compile_definitions(flowrt_runtime_zenoh_smoke PRIVATE FLOWRT_HAS_ZENOH_CXX=1 FLOWRT_ENABLE_TEST_HOOKS=1)
    add_test(NAME flowrt_runtime_zenoh_smoke COMMAND flowrt_runtime_zenoh_smoke)
endif()
