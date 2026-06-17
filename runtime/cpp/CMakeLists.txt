cmake_minimum_required(VERSION 3.22)

project(flowrt_runtime_cpp LANGUAGES CXX)

set(FLOWRT_RUNTIME_CPP_VERSION "0.21.3")

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
    VERSION ${FLOWRT_RUNTIME_CPP_VERSION}
    COMPATIBILITY SameMajorVersion
)
install(
    FILES
        ${CMAKE_CURRENT_BINARY_DIR}/flowrt_runtimeConfig.cmake
        ${CMAKE_CURRENT_BINARY_DIR}/flowrt_runtimeConfigVersion.cmake
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/flowrt_runtime
)

enable_testing()

function(flowrt_keep_asserts_enabled target)
    if(MSVC)
        target_compile_options(${target} PRIVATE /UNDEBUG)
    else()
        target_compile_options(${target} PRIVATE -UNDEBUG)
    endif()
endfunction()

add_executable(flowrt_runtime_smoke tests/runtime_smoke.cpp)
target_link_libraries(flowrt_runtime_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_runtime_smoke)
add_test(NAME flowrt_runtime_smoke COMMAND flowrt_runtime_smoke)

add_executable(flowrt_runtime_introspection_smoke tests/introspection_smoke.cpp)
target_link_libraries(flowrt_runtime_introspection_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_runtime_introspection_smoke)
add_test(NAME flowrt_runtime_introspection_smoke COMMAND flowrt_runtime_introspection_smoke)

add_executable(flowrt_service_smoke tests/service_smoke.cpp)
target_link_libraries(flowrt_service_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_service_smoke)
add_test(NAME flowrt_service_smoke COMMAND flowrt_service_smoke)

add_executable(flowrt_operation_smoke tests/operation_smoke.cpp)
target_link_libraries(flowrt_operation_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_operation_smoke)
add_test(NAME flowrt_operation_smoke COMMAND flowrt_operation_smoke)

add_executable(flowrt_frame_descriptor_smoke tests/frame_descriptor_smoke.cpp)
target_link_libraries(flowrt_frame_descriptor_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_frame_descriptor_smoke)
add_test(NAME flowrt_frame_descriptor_smoke COMMAND flowrt_frame_descriptor_smoke)

add_executable(flowrt_wire_smoke tests/wire_smoke.cpp)
target_link_libraries(flowrt_wire_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_wire_smoke)
add_test(NAME flowrt_wire_smoke COMMAND flowrt_wire_smoke)

add_executable(flowrt_inproc_service_smoke tests/inproc_service_smoke.cpp)
target_link_libraries(flowrt_inproc_service_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_inproc_service_smoke)
add_test(NAME flowrt_inproc_service_smoke COMMAND flowrt_inproc_service_smoke)

add_executable(flowrt_replay_smoke tests/replay_smoke.cpp)
target_link_libraries(flowrt_replay_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_replay_smoke)
add_test(NAME flowrt_replay_smoke COMMAND flowrt_replay_smoke)

add_executable(flowrt_synchronizer_smoke tests/synchronizer_smoke.cpp)
target_link_libraries(flowrt_synchronizer_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_synchronizer_smoke)
add_test(NAME flowrt_synchronizer_smoke COMMAND flowrt_synchronizer_smoke)

add_executable(flowrt_lifecycle_smoke tests/lifecycle_smoke.cpp)
target_link_libraries(flowrt_lifecycle_smoke PRIVATE flowrt_runtime)
flowrt_keep_asserts_enabled(flowrt_lifecycle_smoke)
add_test(NAME flowrt_lifecycle_smoke COMMAND flowrt_lifecycle_smoke)

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
    flowrt_keep_asserts_enabled(flowrt_runtime_iox2_smoke)
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
    flowrt_keep_asserts_enabled(flowrt_runtime_zenoh_smoke)
    add_test(NAME flowrt_runtime_zenoh_smoke COMMAND flowrt_runtime_zenoh_smoke)

    add_executable(flowrt_zenoh_service_smoke tests/zenoh_service_smoke.cpp)
    target_link_libraries(flowrt_zenoh_service_smoke PRIVATE flowrt_runtime ${FLOWRT_ZENOH_CXX_TARGET})
    target_compile_definitions(flowrt_zenoh_service_smoke PRIVATE FLOWRT_HAS_ZENOH_CXX=1)
    flowrt_keep_asserts_enabled(flowrt_zenoh_service_smoke)
    add_test(NAME flowrt_zenoh_service_smoke COMMAND flowrt_zenoh_service_smoke)

    add_executable(flowrt_zenoh_service_server tests/zenoh_service_server.cpp)
    target_link_libraries(flowrt_zenoh_service_server PRIVATE flowrt_runtime ${FLOWRT_ZENOH_CXX_TARGET})
    target_compile_definitions(flowrt_zenoh_service_server PRIVATE FLOWRT_HAS_ZENOH_CXX=1)
    flowrt_keep_asserts_enabled(flowrt_zenoh_service_server)

    add_executable(flowrt_zenoh_service_client tests/zenoh_service_client.cpp)
    target_link_libraries(flowrt_zenoh_service_client PRIVATE flowrt_runtime ${FLOWRT_ZENOH_CXX_TARGET})
    target_compile_definitions(flowrt_zenoh_service_client PRIVATE FLOWRT_HAS_ZENOH_CXX=1)
    flowrt_keep_asserts_enabled(flowrt_zenoh_service_client)
endif()
