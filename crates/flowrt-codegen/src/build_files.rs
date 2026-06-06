use flowrt_ir::{ContractIr, LanguageKind};

use crate::ros2_bridge::ros2_bridge_stem;
use crate::runtime_plan::{contract_backend_features, contract_uses_backend};
use crate::{
    contract_has_ros2_bridge, fixed_message_abi_expectations, has_language, sanitize_package_name,
};

pub(super) fn emit_cmake(contract: &ContractIr) -> String {
    let package_name = sanitize_package_name(&contract.package.name);
    let has_cpp_components = has_language(contract, LanguageKind::Cpp);
    let has_ros2_bridge = contract_has_ros2_bridge(contract);
    let has_cpp_runtime = has_cpp_components || has_ros2_bridge;
    let mut output = format!(
        "# FlowRT 管理产物。不要手工修改。\ncmake_minimum_required(VERSION 3.22)\nproject({}_flowrt_app LANGUAGES CXX)\n\nset(CMAKE_EXPORT_COMPILE_COMMANDS ON)\n\nadd_library({}_flowrt_app INTERFACE)\ntarget_compile_features({}_flowrt_app INTERFACE cxx_std_20)\ntarget_include_directories({}_flowrt_app INTERFACE ${{CMAKE_CURRENT_LIST_DIR}}/../cpp/include)\n",
        package_name, package_name, package_name, package_name
    );

    if has_cpp_runtime {
        let shell_target = format!("{}_cpp_shell", package_name.replace('-', "_"));
        let app_target = format!("{}_cpp_app", package_name.replace('-', "_"));
        output.push_str(
            "\nset(FLOWRT_CPP_RUNTIME_DIR \"\" CACHE PATH \"FlowRT C++ runtime root containing include/flowrt/runtime.hpp\")\n",
        );
        output.push_str(
            "if(FLOWRT_CPP_RUNTIME_DIR)\n    list(PREPEND CMAKE_PREFIX_PATH \"${FLOWRT_CPP_RUNTIME_DIR}\")\n    list(PREPEND CMAKE_BUILD_RPATH \"${FLOWRT_CPP_RUNTIME_DIR}/lib\")\nendif()\n",
        );
        if contract_uses_backend(contract, "iox2") {
            output.push_str(cmake_iox2_dependency_block());
            output.push_str(&format!(
                "target_link_libraries({package_name}_flowrt_app INTERFACE iceoryx2-cxx::static-lib-cxx)\n"
            ));
            output.push_str(&format!(
                "target_compile_definitions({package_name}_flowrt_app INTERFACE FLOWRT_HAS_ICEORYX2_CXX=1)\n"
            ));
        }
        if has_cpp_components && contract_uses_backend(contract, "zenoh") {
            output.push_str(cmake_zenoh_dependency_block());
            output.push_str(&format!(
                "target_link_libraries({package_name}_flowrt_app INTERFACE ${{FLOWRT_ZENOH_CXX_TARGET}})\n"
            ));
            output.push_str(&format!(
                "target_compile_definitions({package_name}_flowrt_app INTERFACE FLOWRT_HAS_ZENOH_CXX=1)\n"
            ));
        }
        output.push_str(
            "if(NOT FLOWRT_CPP_RUNTIME_DIR)\n    find_package(flowrt_runtime 0.1 QUIET)\nendif()\n",
        );
        output.push_str(
            "if(NOT TARGET flowrt::runtime AND NOT FLOWRT_CPP_RUNTIME_DIR)\n    get_filename_component(_flowrt_repo_runtime \"${CMAKE_CURRENT_LIST_DIR}/../../../../runtime/cpp\" ABSOLUTE)\n    if(EXISTS \"${_flowrt_repo_runtime}/include/flowrt/runtime.hpp\")\n        set(FLOWRT_CPP_RUNTIME_DIR \"${_flowrt_repo_runtime}\")\n    endif()\nendif()\n",
        );
        output.push_str(
            "if(FLOWRT_CPP_RUNTIME_DIR)\n    if(NOT EXISTS \"${FLOWRT_CPP_RUNTIME_DIR}/include/flowrt/runtime.hpp\")\n        message(FATAL_ERROR \"FLOWRT_CPP_RUNTIME_DIR does not contain include/flowrt/runtime.hpp: ${FLOWRT_CPP_RUNTIME_DIR}\")\n    endif()\n    target_include_directories({package_name}_flowrt_app INTERFACE ${FLOWRT_CPP_RUNTIME_DIR}/include)\nelseif(TARGET flowrt::runtime)\n    target_link_libraries({package_name}_flowrt_app INTERFACE flowrt::runtime)\nelse()\n    message(FATAL_ERROR \"FlowRT C++ runtime was not found. Install flowrt_runtime, set CMAKE_PREFIX_PATH, or set FLOWRT_CPP_RUNTIME_DIR to a FlowRT runtime/cpp tree.\")\nendif()\n",
        );
        output = output.replace("{package_name}", &package_name);
        if has_cpp_components {
            output.push_str(&format!(
                "\nadd_library({shell_target} STATIC ../cpp/src/runtime_shell.cpp ../cpp/src/selfdesc.cpp)\n"
            ));
            output.push_str(&format!(
                "target_link_libraries({shell_target} PUBLIC {package_name}_flowrt_app)\n"
            ));
            output.push_str(
                "\nfile(GLOB FLOWRT_DEFAULT_USER_CPP_SOURCES CONFIGURE_DEPENDS \"${CMAKE_CURRENT_LIST_DIR}/../../src/cpp/*.cpp\")\nset(FLOWRT_USER_CPP_SOURCES ${FLOWRT_DEFAULT_USER_CPP_SOURCES} CACHE STRING \"User C++ sources that implement flowrt_user::build_app\")\n",
            );
            output.push_str("if(FLOWRT_USER_CPP_SOURCES)\n");
            let user_target = format!("{}_cpp_user", package_name.replace('-', "_"));
            output.push_str(&format!(
                "    add_library({user_target} STATIC ${{FLOWRT_USER_CPP_SOURCES}})\n"
            ));
            output.push_str(&format!(
                "    target_link_libraries({user_target} PUBLIC {package_name}_flowrt_app)\n"
            ));
            output.push_str(&format!(
                "    add_executable({app_target} ../cpp/src/main.cpp)\n"
            ));
            output.push_str(&format!(
                "    target_link_libraries({app_target} PRIVATE {shell_target} {user_target})\n"
            ));
            output.push_str("endif()\n");
        }
        if has_ros2_bridge {
            let bridge_target = ros2_bridge_stem(contract);
            output.push_str(cmake_ros2_bridge_dependency_block());
            output.push_str(&format!(
                "\nadd_executable({bridge_target} ../cpp/src/ros2_bridge.cpp)\n"
            ));
            output.push_str(&cmake_ros2_bridge_target_block(&bridge_target));
        }
    }

    let has_fixed_abi_tests = fixed_message_abi_expectations(contract)
        .map(|expectations| !expectations.is_empty())
        .unwrap_or(false);
    if has_cpp_runtime && has_fixed_abi_tests {
        let test_target = format!("{}_message_abi", package_name.replace('-', "_"));
        output.push_str("\ninclude(CTest)\nif(BUILD_TESTING)\n");
        output.push_str(&format!(
            "    add_executable({test_target} ../cpp/tests/message_abi.cpp)\n"
        ));
        output.push_str(&format!(
            "    target_link_libraries({test_target} PRIVATE {package_name}_flowrt_app)\n"
        ));
        output.push_str(
            "    set(FLOWRT_ABI_CPP_FIXTURE_DIR \"${CMAKE_CURRENT_LIST_DIR}/abi-fixtures/cpp\")\n",
        );
        output.push_str(&format!(
            "    target_compile_definitions({test_target} PRIVATE FLOWRT_ABI_FIXTURE_DIR=\"${{FLOWRT_ABI_CPP_FIXTURE_DIR}}\")\n"
        ));
        output.push_str(&format!(
            "    add_custom_command(TARGET {test_target} POST_BUILD\n        COMMAND $<TARGET_FILE:{test_target}>\n        COMMENT \"Generate C++ Message ABI cross-language fixtures\")\n"
        ));
        output.push_str(&format!(
            "    add_test(NAME message_abi COMMAND {test_target})\n"
        ));
        output.push_str("endif()\n");
    }

    output
}

pub(super) fn emit_cargo_manifest(contract: &ContractIr) -> String {
    let package_name = sanitize_package_name(&contract.package.name).replace('_', "-");
    let has_rust = has_language(contract, LanguageKind::Rust);
    let has_supervisor =
        has_rust || has_language(contract, LanguageKind::Cpp) || contract_has_ros2_bridge(contract);
    let mut output = format!(
        "# FlowRT 管理产物。不要手工修改。\n[package]\nname = \"{}-flowrt-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n[lib]\nname = \"flowrt_app\"\npath = \"../rust/src/lib.rs\"\n\n[dependencies]\n",
        package_name
    );
    let mut bins = String::new();

    if has_rust {
        let backend_features = contract_backend_features(contract);
        if backend_features.is_empty() {
            output.push_str("flowrt = { version = \"0.1\" }");
        } else {
            let features = backend_features
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!(
                "flowrt = {{ version = \"0.1\", features = [{features}] }}"
            ));
        }
        output.push('\n');
        bins.push_str(&format!(
            "\n[[bin]]\nname = \"{}-flowrt-app\"\npath = \"../rust/src/main.rs\"\n",
            package_name
        ));
    } else if has_supervisor {
        output.push_str("flowrt = { version = \"0.1\" }\n");
    }

    if has_supervisor {
        output
            .push_str("serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n");
        bins.push_str(&format!(
            "\n[[bin]]\nname = \"{}-flowrt-supervisor\"\npath = \"../rust/src/supervisor_main.rs\"\n",
            package_name
        ));
    }
    output.push_str(&bins);

    let has_fixed_abi_tests = fixed_message_abi_expectations(contract)
        .map(|expectations| !expectations.is_empty())
        .unwrap_or(false);
    if has_rust && has_fixed_abi_tests {
        output.push_str(
            "\n[[test]]\nname = \"message_abi\"\npath = \"../rust/tests/message_abi.rs\"\n",
        );
    }

    output
}

fn cmake_iox2_dependency_block() -> &'static str {
    r#"
find_package(iceoryx2-cxx 0.9.1 QUIET)
if(NOT TARGET iceoryx2-cxx::static-lib-cxx)
  message(FATAL_ERROR "iceoryx2-cxx 0.9.1 was not found. Install the FlowRT package or set FLOWRT_CPP_RUNTIME_DIR/CMAKE_PREFIX_PATH to a FlowRT private prefix.")
endif()
"#
}

fn cmake_zenoh_dependency_block() -> &'static str {
    r#"
set(FLOWRT_ZENOH_CXX_TARGET "")
find_package(zenohc 1.9.0 QUIET)
find_package(zenohcxx 1.9.0 QUIET)
if(TARGET zenohcxx::zenohc)
  set(FLOWRT_ZENOH_CXX_TARGET zenohcxx::zenohc)
endif()
if(NOT FLOWRT_ZENOH_CXX_TARGET)
  message(FATAL_ERROR "zenoh C++ target is unavailable. Install zenoh-c and zenoh-cpp 1.9.0 so CMake exposes zenohc::lib and zenohcxx::zenohc, then set CMAKE_PREFIX_PATH if needed.")
endif()
"#
}

fn cmake_ros2_bridge_dependency_block() -> &'static str {
    r#"
if(DEFINED ENV{AMENT_PREFIX_PATH})
  cmake_path(CONVERT "$ENV{AMENT_PREFIX_PATH}" TO_CMAKE_PATH_LIST FLOWRT_AMENT_PREFIX_PATH)
  list(PREPEND CMAKE_PREFIX_PATH ${FLOWRT_AMENT_PREFIX_PATH})
endif()
find_package(rclcpp REQUIRED)
find_package(std_msgs REQUIRED)
find_package(rosidl_typesupport_cpp REQUIRED)
find_package(rmw_zenoh_cpp REQUIRED)
find_package(zenoh_cpp_vendor REQUIRED)
get_filename_component(FLOWRT_ROS2_ZENOH_VENDOR_PREFIX "${zenoh_cpp_vendor_DIR}/../../../opt/zenoh_cpp_vendor" ABSOLUTE)
set(FLOWRT_ROS2_ZENOH_INCLUDE "${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}/include")
set(FLOWRT_ROS2_ZENOH_LIB "${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}/lib/libzenohc.so")
if(NOT EXISTS "${FLOWRT_ROS2_ZENOH_INCLUDE}/zenoh.hxx" OR NOT EXISTS "${FLOWRT_ROS2_ZENOH_LIB}")
  message(FATAL_ERROR "rmw_zenoh_cpp must provide zenoh_cpp_vendor headers and libzenohc.so under ${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}. Install the ROS2 zenoh RMW package for the selected ROS2 distribution.")
endif()
"#
}

fn cmake_ros2_bridge_target_block(bridge_target: &str) -> String {
    format!(
        r#"target_compile_features({bridge_target} PRIVATE cxx_std_20)
target_include_directories({bridge_target} PRIVATE ${{CMAKE_CURRENT_LIST_DIR}}/../cpp/include)
if(FLOWRT_CPP_RUNTIME_DIR)
  target_include_directories({bridge_target} PRIVATE ${{FLOWRT_CPP_RUNTIME_DIR}}/include)
elseif(TARGET flowrt::runtime)
  target_link_libraries({bridge_target} PRIVATE flowrt::runtime)
else()
  message(FATAL_ERROR "FlowRT C++ runtime headers were not found for ROS2 bridge target.")
endif()
target_include_directories({bridge_target} BEFORE PRIVATE ${{FLOWRT_ROS2_ZENOH_INCLUDE}})
target_compile_definitions({bridge_target} PRIVATE ZENOHCXX_ZENOHC)
target_link_libraries({bridge_target} PRIVATE rclcpp::rclcpp std_msgs::std_msgs__rosidl_typesupport_cpp rosidl_typesupport_cpp::rosidl_typesupport_cpp ${{FLOWRT_ROS2_ZENOH_LIB}})
set_property(TARGET {bridge_target} APPEND PROPERTY BUILD_RPATH "${{FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}}/lib")
"#
    )
}
