cmake_minimum_required(VERSION 3.22)

project(flowrt_runtime_cpp LANGUAGES CXX)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

include(FetchContent)

find_package(Threads REQUIRED)

add_library(flowrt_runtime INTERFACE)
target_include_directories(flowrt_runtime INTERFACE ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_compile_features(flowrt_runtime INTERFACE cxx_std_20)
target_link_libraries(flowrt_runtime INTERFACE Threads::Threads)

enable_testing()

add_executable(flowrt_runtime_smoke tests/runtime_smoke.cpp)
target_link_libraries(flowrt_runtime_smoke PRIVATE flowrt_runtime)
add_test(NAME flowrt_runtime_smoke COMMAND flowrt_runtime_smoke)

add_executable(flowrt_runtime_introspection_smoke tests/introspection_smoke.cpp)
target_link_libraries(flowrt_runtime_introspection_smoke PRIVATE flowrt_runtime)
add_test(NAME flowrt_runtime_introspection_smoke COMMAND flowrt_runtime_introspection_smoke)

option(FLOWRT_CPP_ENABLE_IOX2_TESTS "Build FlowRT C++ runtime tests that require iceoryx2-cxx" OFF)
option(FLOWRT_CPP_ENABLE_ZENOH_TESTS "Build FlowRT C++ runtime tests that require zenoh-cpp" OFF)
option(FLOWRT_FETCH_IOX2 "Download and build iceoryx2-cxx v0.9.1 when it is not installed" ON)

if(FLOWRT_CPP_ENABLE_IOX2_TESTS)
    find_package(iceoryx2-cxx 0.9.1 QUIET)
    if(NOT TARGET iceoryx2-cxx::static-lib-cxx)
        if(NOT FLOWRT_FETCH_IOX2)
            message(FATAL_ERROR "iceoryx2-cxx 0.9.1 was not found. Install it, set CMAKE_PREFIX_PATH, or enable FLOWRT_FETCH_IOX2.")
        endif()
        set(BUILD_CXX ON CACHE BOOL "Build iceoryx2 C++ bindings" FORCE)
        set(BUILD_EXAMPLES OFF CACHE BOOL "Build iceoryx2 examples" FORCE)
        FetchContent_Declare(
            iceoryx2
            GIT_REPOSITORY https://github.com/eclipse-iceoryx/iceoryx2.git
            GIT_TAG v0.9.1
            GIT_SHALLOW TRUE
        )
        FetchContent_MakeAvailable(iceoryx2)
    endif()
    if(NOT TARGET iceoryx2-cxx::static-lib-cxx)
        message(FATAL_ERROR "iceoryx2-cxx::static-lib-cxx target is unavailable after dependency resolution")
    endif()

    add_executable(flowrt_runtime_iox2_smoke tests/iox2_smoke.cpp)
    target_link_libraries(flowrt_runtime_iox2_smoke PRIVATE flowrt_runtime iceoryx2-cxx::static-lib-cxx)
    target_compile_definitions(flowrt_runtime_iox2_smoke PRIVATE FLOWRT_HAS_ICEORYX2_CXX=1)
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
    target_compile_definitions(flowrt_runtime_zenoh_smoke PRIVATE FLOWRT_HAS_ZENOH_CXX=1)
    add_test(NAME flowrt_runtime_zenoh_smoke COMMAND flowrt_runtime_zenoh_smoke)
endif()
