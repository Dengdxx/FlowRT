# FlowRT 管理产物。不要手工修改。
cmake_minimum_required(VERSION 3.22)
project(graph_demo_flowrt_app LANGUAGES C CXX)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(graph_demo_flowrt_app INTERFACE)
target_compile_features(graph_demo_flowrt_app INTERFACE cxx_std_20)
target_include_directories(graph_demo_flowrt_app INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../cpp/include)
