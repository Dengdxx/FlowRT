# FlowRT 管理产物。不要手工修改。
cmake_minimum_required(VERSION 3.22)
project(xproc_feedback_cpp_flowrt_app LANGUAGES C CXX)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_library(xproc_feedback_cpp_flowrt_app INTERFACE)
target_compile_features(xproc_feedback_cpp_flowrt_app INTERFACE cxx_std_20)
target_include_directories(xproc_feedback_cpp_flowrt_app INTERFACE ${CMAKE_CURRENT_LIST_DIR}/../cpp/include)

set(FLOWRT_CPP_RUNTIME_DIR "" CACHE PATH "FlowRT C++ runtime root containing include/flowrt/runtime.hpp")
set(FLOWRT_CXX_COMPILE_OPTIONS "" CACHE STRING "Extra C++ compile options from the FlowRT toolchain profile")
set(FLOWRT_EXE_LINK_OPTIONS "" CACHE STRING "Extra executable link options from the FlowRT toolchain profile")
set(FLOWRT_EXE_LINK_LIBRARIES "" CACHE STRING "Extra executable link libraries from the FlowRT toolchain profile")
if(FLOWRT_CPP_RUNTIME_DIR)
    list(PREPEND CMAKE_PREFIX_PATH "${FLOWRT_CPP_RUNTIME_DIR}")
    list(PREPEND CMAKE_BUILD_RPATH "${FLOWRT_CPP_RUNTIME_DIR}/lib")
endif()

set(FLOWRT_ZENOH_CXX_TARGET "")
find_package(zenohc 1.9.0 QUIET)
find_package(zenohcxx 1.9.0 QUIET)
if(TARGET zenohcxx::zenohc)
  set(FLOWRT_ZENOH_CXX_TARGET zenohcxx::zenohc)
endif()
if(NOT FLOWRT_ZENOH_CXX_TARGET)
  message(FATAL_ERROR "zenoh C++ target is unavailable. Install zenoh-c and zenoh-cpp 1.9.0 so CMake exposes zenohc::lib and zenohcxx::zenohc, then set CMAKE_PREFIX_PATH if needed.")
endif()
target_link_libraries(xproc_feedback_cpp_flowrt_app INTERFACE ${FLOWRT_ZENOH_CXX_TARGET})
target_compile_definitions(xproc_feedback_cpp_flowrt_app INTERFACE FLOWRT_HAS_ZENOH_CXX=1)
if(NOT FLOWRT_CPP_RUNTIME_DIR)
    find_package(flowrt_runtime 0.28 QUIET)
endif()
option(FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK "Allow falling back to FlowRT source tree runtime/cpp (dev mode only)" OFF)
if(NOT TARGET flowrt::runtime AND NOT FLOWRT_CPP_RUNTIME_DIR AND FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK)
    get_filename_component(_flowrt_repo_runtime "${CMAKE_CURRENT_LIST_DIR}/../../../../runtime/cpp" ABSOLUTE)
    if(EXISTS "${_flowrt_repo_runtime}/include/flowrt/runtime.hpp")
        set(FLOWRT_CPP_RUNTIME_DIR "${_flowrt_repo_runtime}")
    endif()
endif()
if(FLOWRT_CPP_RUNTIME_DIR)
    if(NOT EXISTS "${FLOWRT_CPP_RUNTIME_DIR}/include/flowrt/runtime.hpp")
        message(FATAL_ERROR "FLOWRT_CPP_RUNTIME_DIR does not contain include/flowrt/runtime.hpp: ${FLOWRT_CPP_RUNTIME_DIR}")
    endif()
    target_include_directories(xproc_feedback_cpp_flowrt_app INTERFACE ${FLOWRT_CPP_RUNTIME_DIR}/include)
elseif(TARGET flowrt::runtime)
    target_link_libraries(xproc_feedback_cpp_flowrt_app INTERFACE flowrt::runtime)
else()
    message(FATAL_ERROR "FlowRT C++ runtime was not found. Install the FlowRT package, set CMAKE_PREFIX_PATH, or set FLOWRT_CPP_RUNTIME_DIR to a FlowRT runtime/cpp tree. If developing inside the FlowRT source tree, set -DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON.")
endif()

add_library(xproc_feedback_cpp_cpp_shell STATIC ../cpp/src/runtime_shell.cpp ../cpp/src/selfdesc.cpp)
target_link_libraries(xproc_feedback_cpp_cpp_shell PUBLIC xproc_feedback_cpp_flowrt_app)

set(FLOWRT_USER_APP_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../app")
set(FLOWRT_USER_CPP_ROOT "${FLOWRT_USER_APP_ROOT}/cpp")
set(FLOWRT_USER_C_ROOT "${FLOWRT_USER_APP_ROOT}/c")
file(GLOB_RECURSE FLOWRT_DEFAULT_USER_CPP_SOURCES CONFIGURE_DEPENDS
    "${FLOWRT_USER_CPP_ROOT}/*.cpp"
    "${FLOWRT_USER_CPP_ROOT}/*.cc"
    "${FLOWRT_USER_CPP_ROOT}/*.cxx"
    "${FLOWRT_USER_CPP_ROOT}/*.c"
    "${FLOWRT_USER_C_ROOT}/*.c"
    "${FLOWRT_USER_APP_ROOT}/*/cpp/*.cpp"
    "${FLOWRT_USER_APP_ROOT}/*/cpp/*.cc"
    "${FLOWRT_USER_APP_ROOT}/*/cpp/*.cxx"
    "${FLOWRT_USER_APP_ROOT}/*/cpp/*.c"
    "${FLOWRT_USER_APP_ROOT}/*/c/*.c"
)
set(FLOWRT_USER_CPP_SOURCES ${FLOWRT_DEFAULT_USER_CPP_SOURCES} CACHE STRING "User C/C++ sources from app/cpp, app/c, app/<module>/cpp and app/<module>/c that implement flowrt_user::build_app and C callback factories")
if(FLOWRT_USER_CPP_SOURCES)
    add_library(xproc_feedback_cpp_cpp_user STATIC ${FLOWRT_USER_CPP_SOURCES})
    target_include_directories(xproc_feedback_cpp_cpp_user PUBLIC ${FLOWRT_USER_CPP_ROOT} ${FLOWRT_USER_C_ROOT} ${FLOWRT_USER_APP_ROOT})
    if(FLOWRT_CXX_COMPILE_OPTIONS)
        target_compile_options(xproc_feedback_cpp_cpp_user PRIVATE ${FLOWRT_CXX_COMPILE_OPTIONS})
    endif()
    target_link_libraries(xproc_feedback_cpp_cpp_user PUBLIC xproc_feedback_cpp_flowrt_app)
    add_executable(xproc_feedback_cpp_cpp_app ../cpp/src/main.cpp)
    if(FLOWRT_EXE_LINK_OPTIONS)
        target_link_options(xproc_feedback_cpp_cpp_app PRIVATE ${FLOWRT_EXE_LINK_OPTIONS})
    endif()
    target_link_libraries(xproc_feedback_cpp_cpp_app PRIVATE xproc_feedback_cpp_cpp_shell xproc_feedback_cpp_cpp_user)
    if(FLOWRT_EXE_LINK_LIBRARIES)
        target_link_libraries(xproc_feedback_cpp_cpp_app PRIVATE ${FLOWRT_EXE_LINK_LIBRARIES})
    endif()
endif()

include(CTest)
if(BUILD_TESTING)
    add_executable(xproc_feedback_cpp_message_abi ../cpp/tests/message_abi.cpp)
    target_link_libraries(xproc_feedback_cpp_message_abi PRIVATE xproc_feedback_cpp_flowrt_app)
    set(FLOWRT_ABI_CPP_FIXTURE_DIR "${CMAKE_CURRENT_LIST_DIR}/abi-fixtures/cpp")
    target_compile_definitions(xproc_feedback_cpp_message_abi PRIVATE FLOWRT_ABI_FIXTURE_DIR="${FLOWRT_ABI_CPP_FIXTURE_DIR}")
    if(NOT CMAKE_CROSSCOMPILING)
        add_custom_command(TARGET xproc_feedback_cpp_message_abi POST_BUILD
            COMMAND $<TARGET_FILE:xproc_feedback_cpp_message_abi>
            COMMENT "Generate C++ Message ABI cross-language fixtures")
        add_test(NAME message_abi COMMAND xproc_feedback_cpp_message_abi)
    else()
        message(STATUS "Skipping C++ Message ABI fixture execution while cross compiling")
    endif()
endif()
