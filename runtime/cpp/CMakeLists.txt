cmake_minimum_required(VERSION 3.20)

project(flowrt_runtime_cpp LANGUAGES CXX)

add_library(flowrt_runtime INTERFACE)
target_include_directories(flowrt_runtime INTERFACE ${CMAKE_CURRENT_SOURCE_DIR}/include)
target_compile_features(flowrt_runtime INTERFACE cxx_std_20)

enable_testing()

add_executable(flowrt_runtime_smoke tests/runtime_smoke.cpp)
target_link_libraries(flowrt_runtime_smoke PRIVATE flowrt_runtime)
add_test(NAME flowrt_runtime_smoke COMMAND flowrt_runtime_smoke)

option(FLOWRT_CPP_ENABLE_IOX2_TESTS "Build FlowRT C++ runtime tests that require iceoryx2-cxx" OFF)

if(FLOWRT_CPP_ENABLE_IOX2_TESTS)
    find_package(iceoryx2-cxx 0.9.1 REQUIRED)

    add_executable(flowrt_runtime_iox2_smoke tests/iox2_smoke.cpp)
    target_link_libraries(flowrt_runtime_iox2_smoke PRIVATE flowrt_runtime iceoryx2-cxx::static-lib-cxx)
    target_compile_definitions(flowrt_runtime_iox2_smoke PRIVATE FLOWRT_HAS_ICEORYX2_CXX=1)
    add_test(NAME flowrt_runtime_iox2_smoke COMMAND flowrt_runtime_iox2_smoke)
endif()
