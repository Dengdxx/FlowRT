use std::collections::BTreeSet;

use flowrt_ir::{ContractIr, LanguageKind};

use crate::ros2_bridge::ros2_bridge_stem;
use crate::runtime_plan::{
    contract_backend_features, contract_has_operations_for_language, contract_uses_backend,
};
use crate::{
    contract_has_external_process, contract_has_ros2_bridge, contract_has_variable_messages,
    fixed_message_abi_expectations, has_language, sanitize_package_name,
};

const FLOWRT_RUNTIME_VERSION_REQ: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);

pub(super) fn emit_cmake(contract: &ContractIr) -> String {
    let package_name = sanitize_package_name(&contract.package.name);
    let has_cpp_components = has_language(contract, LanguageKind::Cpp);
    let has_c_components = has_language(contract, LanguageKind::C);
    let has_cpp_shell_components = has_cpp_components || has_c_components;
    let has_ros2_bridge = contract_has_ros2_bridge(contract);
    let has_cpp_runtime = has_cpp_shell_components || has_ros2_bridge;
    let mut output = format!(
        "# FlowRT 管理产物。不要手工修改。\ncmake_minimum_required(VERSION 3.22)\nproject({package_name}_flowrt_app LANGUAGES C CXX)\n\nset(CMAKE_EXPORT_COMPILE_COMMANDS ON)\n\nadd_library({package_name}_flowrt_app INTERFACE)\ntarget_compile_features({package_name}_flowrt_app INTERFACE cxx_std_20)\ntarget_include_directories({package_name}_flowrt_app INTERFACE ${{CMAKE_CURRENT_LIST_DIR}}/../cpp/include)\n"
    );

    if has_cpp_runtime {
        let shell_target = format!("{}_cpp_shell", package_name.replace('-', "_"));
        let app_target = format!("{}_cpp_app", package_name.replace('-', "_"));
        let cpp_pkg_config_packages = cpp_component_pkg_config_packages(contract);
        output.push_str(
            "\nset(FLOWRT_CPP_RUNTIME_DIR \"\" CACHE PATH \"FlowRT C++ runtime root containing include/flowrt/runtime.hpp\")\n",
        );
        output.push_str(
            "set(FLOWRT_CXX_COMPILE_OPTIONS \"\" CACHE STRING \"Extra C++ compile options from the FlowRT toolchain profile\")\nset(FLOWRT_EXE_LINK_OPTIONS \"\" CACHE STRING \"Extra executable link options from the FlowRT toolchain profile\")\nset(FLOWRT_EXE_LINK_LIBRARIES \"\" CACHE STRING \"Extra executable link libraries from the FlowRT toolchain profile\")\n",
        );
        output.push_str(
            "if(FLOWRT_CPP_RUNTIME_DIR)\n    list(PREPEND CMAKE_PREFIX_PATH \"${FLOWRT_CPP_RUNTIME_DIR}\")\n    list(PREPEND CMAKE_BUILD_RPATH \"${FLOWRT_CPP_RUNTIME_DIR}/lib\")\nendif()\n",
        );
        if !cpp_pkg_config_packages.is_empty() {
            output.push_str(&cmake_pkg_config_dependency_block(&cpp_pkg_config_packages));
        }
        if contract_uses_backend(contract, "iox2") {
            output.push_str(cmake_iox2_dependency_block());
            output.push_str(&format!(
                "target_link_libraries({package_name}_flowrt_app INTERFACE iceoryx2-cxx::static-lib-cxx)\n"
            ));
            output.push_str(&format!(
                "target_compile_definitions({package_name}_flowrt_app INTERFACE FLOWRT_HAS_ICEORYX2_CXX=1)\n"
            ));
        }
        let cpp_requires_zenoh_backend =
            has_cpp_shell_components && contract_uses_backend(contract, "zenoh");
        let cpp_wants_remote_operation_control = has_cpp_shell_components
            && (contract_has_operations_for_language(contract, LanguageKind::Cpp)
                || contract_has_operations_for_language(contract, LanguageKind::C));
        if cpp_requires_zenoh_backend {
            output.push_str(cmake_required_zenoh_dependency_block());
            output.push_str(&format!(
                "target_link_libraries({package_name}_flowrt_app INTERFACE ${{FLOWRT_ZENOH_CXX_TARGET}})\n"
            ));
            output.push_str(&format!(
                "target_compile_definitions({package_name}_flowrt_app INTERFACE FLOWRT_HAS_ZENOH_CXX=1)\n"
            ));
        } else if cpp_wants_remote_operation_control {
            output.push_str(cmake_optional_zenoh_dependency_block());
            output.push_str(&format!(
                "if(FLOWRT_ZENOH_CXX_TARGET)\n  target_link_libraries({package_name}_flowrt_app INTERFACE ${{FLOWRT_ZENOH_CXX_TARGET}})\n  target_compile_definitions({package_name}_flowrt_app INTERFACE FLOWRT_HAS_ZENOH_CXX=1)\nendif()\n"
            ));
        }
        output.push_str(
            "if(NOT FLOWRT_CPP_RUNTIME_DIR)\n    find_package(flowrt_runtime {flowrt_version_req} QUIET)\nendif()\n",
        );
        output.push_str(
            "option(FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK \"Allow falling back to FlowRT source tree runtime/cpp (dev mode only)\" OFF)\n",
        );
        output.push_str(
            "if(NOT TARGET flowrt::runtime AND NOT FLOWRT_CPP_RUNTIME_DIR AND FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK)\n    get_filename_component(_flowrt_repo_runtime \"${CMAKE_CURRENT_LIST_DIR}/../../../../runtime/cpp\" ABSOLUTE)\n    if(EXISTS \"${_flowrt_repo_runtime}/include/flowrt/runtime.hpp\")\n        set(FLOWRT_CPP_RUNTIME_DIR \"${_flowrt_repo_runtime}\")\n    endif()\nendif()\n",
        );
        output.push_str(
            "if(FLOWRT_CPP_RUNTIME_DIR)\n    if(NOT EXISTS \"${FLOWRT_CPP_RUNTIME_DIR}/include/flowrt/runtime.hpp\")\n        message(FATAL_ERROR \"FLOWRT_CPP_RUNTIME_DIR does not contain include/flowrt/runtime.hpp: ${FLOWRT_CPP_RUNTIME_DIR}\")\n    endif()\n    target_include_directories({package_name}_flowrt_app INTERFACE ${FLOWRT_CPP_RUNTIME_DIR}/include)\nelseif(TARGET flowrt::runtime)\n    target_link_libraries({package_name}_flowrt_app INTERFACE flowrt::runtime)\nelse()\n    message(FATAL_ERROR \"FlowRT C++ runtime was not found. Install the FlowRT package, set CMAKE_PREFIX_PATH, or set FLOWRT_CPP_RUNTIME_DIR to a FlowRT runtime/cpp tree. If developing inside the FlowRT source tree, set -DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON.\")\nendif()\n",
        );
        output = output.replace("{package_name}", &package_name);
        output = output.replace("{flowrt_version_req}", FLOWRT_RUNTIME_VERSION_REQ);
        if has_cpp_shell_components {
            output.push_str(&format!(
                "\nadd_library({shell_target} STATIC ../cpp/src/runtime_shell.cpp ../cpp/src/selfdesc.cpp)\n"
            ));
            output.push_str(&format!(
                "target_link_libraries({shell_target} PUBLIC {package_name}_flowrt_app)\n"
            ));
            output.push_str(
                "\nset(FLOWRT_USER_APP_ROOT \"${CMAKE_CURRENT_LIST_DIR}/../../app\")\nset(FLOWRT_USER_CPP_ROOT \"${FLOWRT_USER_APP_ROOT}/cpp\")\nset(FLOWRT_USER_C_ROOT \"${FLOWRT_USER_APP_ROOT}/c\")\nfile(GLOB_RECURSE FLOWRT_DEFAULT_USER_CPP_SOURCES CONFIGURE_DEPENDS\n    \"${FLOWRT_USER_CPP_ROOT}/*.cpp\"\n    \"${FLOWRT_USER_CPP_ROOT}/*.cc\"\n    \"${FLOWRT_USER_CPP_ROOT}/*.cxx\"\n    \"${FLOWRT_USER_CPP_ROOT}/*.c\"\n    \"${FLOWRT_USER_C_ROOT}/*.c\"\n    \"${FLOWRT_USER_APP_ROOT}/*/cpp/*.cpp\"\n    \"${FLOWRT_USER_APP_ROOT}/*/cpp/*.cc\"\n    \"${FLOWRT_USER_APP_ROOT}/*/cpp/*.cxx\"\n    \"${FLOWRT_USER_APP_ROOT}/*/cpp/*.c\"\n    \"${FLOWRT_USER_APP_ROOT}/*/c/*.c\"\n)\nset(FLOWRT_USER_CPP_SOURCES ${FLOWRT_DEFAULT_USER_CPP_SOURCES} CACHE STRING \"User C/C++ sources from app/cpp, app/c, app/<module>/cpp and app/<module>/c that implement flowrt_user::build_app and C callback factories\")\n",
            );
            output.push_str("if(FLOWRT_USER_CPP_SOURCES)\n");
            let user_target = format!("{}_cpp_user", package_name.replace('-', "_"));
            output.push_str(&format!(
                "    add_library({user_target} STATIC ${{FLOWRT_USER_CPP_SOURCES}})\n"
            ));
            output.push_str(&format!(
                "    target_include_directories({user_target} PUBLIC ${{FLOWRT_USER_CPP_ROOT}} ${{FLOWRT_USER_C_ROOT}} ${{FLOWRT_USER_APP_ROOT}})\n"
            ));
            output.push_str(&format!(
                "    if(FLOWRT_CXX_COMPILE_OPTIONS)\n        target_compile_options({user_target} PRIVATE ${{FLOWRT_CXX_COMPILE_OPTIONS}})\n    endif()\n"
            ));
            output.push_str(&format!(
                "    target_link_libraries({user_target} PUBLIC {package_name}_flowrt_app)\n"
            ));
            if !cpp_pkg_config_packages.is_empty() {
                output.push_str(&format!(
                    "    target_link_libraries({user_target} PUBLIC {})\n",
                    cmake_pkg_config_targets(&cpp_pkg_config_packages).join(" ")
                ));
            }
            output.push_str(&format!(
                "    add_executable({app_target} ../cpp/src/main.cpp)\n"
            ));
            output.push_str(&format!(
                "    if(FLOWRT_EXE_LINK_OPTIONS)\n        target_link_options({app_target} PRIVATE ${{FLOWRT_EXE_LINK_OPTIONS}})\n    endif()\n"
            ));
            output.push_str(&format!(
                "    target_link_libraries({app_target} PRIVATE {shell_target} {user_target})\n"
            ));
            output.push_str(&format!(
                "    if(FLOWRT_EXE_LINK_LIBRARIES)\n        target_link_libraries({app_target} PRIVATE ${{FLOWRT_EXE_LINK_LIBRARIES}})\n    endif()\n"
            ));
            output.push_str("endif()\n");
        }
        if has_ros2_bridge {
            let bridge_target = ros2_bridge_stem(contract);
            let has_pose_bridge = contract_has_ros2_bridge_type(contract, "geometry_msgs/msg/Pose");
            output.push_str(&cmake_ros2_bridge_dependency_block(has_pose_bridge));
            output.push_str(&format!(
                "\nadd_executable({bridge_target} ../cpp/src/ros2_bridge.cpp)\n"
            ));
            output.push_str(&cmake_ros2_bridge_target_block(
                &bridge_target,
                has_pose_bridge,
            ));
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
        output.push_str("    if(NOT CMAKE_CROSSCOMPILING)\n");
        output.push_str(&format!(
            "        add_custom_command(TARGET {test_target} POST_BUILD\n            COMMAND $<TARGET_FILE:{test_target}>\n            COMMENT \"Generate C++ Message ABI cross-language fixtures\")\n"
        ));
        output.push_str(&format!(
            "        add_test(NAME message_abi COMMAND {test_target})\n"
        ));
        output.push_str(
            "    else()\n        message(STATUS \"Skipping C++ Message ABI fixture execution while cross compiling\")\n    endif()\n",
        );
        output.push_str("endif()\n");
    }

    if has_cpp_runtime && contract_has_variable_messages(contract) {
        let test_target = format!("{}_message_frame", package_name.replace('-', "_"));
        output.push_str("\ninclude(CTest)\nif(BUILD_TESTING)\n");
        output.push_str(&format!(
            "    add_executable({test_target} ../cpp/tests/message_frame.cpp)\n"
        ));
        output.push_str(&format!(
            "    target_link_libraries({test_target} PRIVATE {package_name}_flowrt_app)\n"
        ));
        output.push_str(
            "    set(FLOWRT_FRAME_CPP_FIXTURE_DIR \"${CMAKE_CURRENT_LIST_DIR}/abi-fixtures/cpp\")\n",
        );
        output.push_str(&format!(
            "    target_compile_definitions({test_target} PRIVATE FLOWRT_ABI_FIXTURE_DIR=\"${{FLOWRT_FRAME_CPP_FIXTURE_DIR}}\")\n"
        ));
        output.push_str("    if(NOT CMAKE_CROSSCOMPILING)\n");
        output.push_str(&format!(
            "        add_custom_command(TARGET {test_target} POST_BUILD\n            COMMAND $<TARGET_FILE:{test_target}>\n            COMMENT \"Generate C++ variable frame cross-language fixtures\")\n"
        ));
        output.push_str(&format!(
            "        add_test(NAME message_frame COMMAND {test_target})\n"
        ));
        output.push_str(
            "    else()\n        message(STATUS \"Skipping C++ variable frame fixture execution while cross compiling\")\n    endif()\n",
        );
        output.push_str("endif()\n");
    }

    output
}

pub(super) fn emit_cargo_manifest(contract: &ContractIr) -> String {
    let package_name = sanitize_package_name(&contract.package.name).replace('_', "-");
    let has_rust = has_language(contract, LanguageKind::Rust);
    let has_supervisor = has_rust
        || has_language(contract, LanguageKind::C)
        || has_language(contract, LanguageKind::Cpp)
        || contract_has_ros2_bridge(contract)
        || contract_has_external_process(contract);
    let mut output = format!(
        "# FlowRT 管理产物。不要手工修改。\n[package]\nname = \"{package_name}-flowrt-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n[lib]\nname = \"flowrt_app\"\npath = \"../rust/src/lib.rs\"\n\n[dependencies]\n"
    );
    let mut bins = String::new();

    if has_rust {
        let backend_features = contract_backend_features(contract);
        if backend_features.is_empty() {
            output.push_str(&flowrt_dependency(None));
        } else {
            let features = backend_features
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&flowrt_dependency(Some(&features)));
        }
        output.push('\n');
        bins.push_str(&format!(
            "\n[[bin]]\nname = \"{package_name}-flowrt-app\"\npath = \"../rust/src/main.rs\"\n"
        ));
    } else if has_supervisor {
        output.push_str(&flowrt_dependency(None));
        output.push('\n');
    }

    if has_supervisor {
        output
            .push_str("serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n");
        bins.push_str(&format!(
            "\n[[bin]]\nname = \"{package_name}-flowrt-supervisor\"\npath = \"../rust/src/supervisor_main.rs\"\n"
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
    if has_rust && contract_has_variable_messages(contract) {
        output.push_str(
            "\n[[test]]\nname = \"message_frame\"\npath = \"../rust/tests/message_frame.rs\"\n",
        );
    }

    output
}

fn flowrt_dependency(features: Option<&str>) -> String {
    match features {
        Some(features) => format!(
            "flowrt = {{ version = \"{FLOWRT_RUNTIME_VERSION_REQ}\", features = [{features}] }}"
        ),
        None => format!("flowrt = {{ version = \"{FLOWRT_RUNTIME_VERSION_REQ}\" }}"),
    }
}

fn cpp_component_pkg_config_packages(contract: &ContractIr) -> Vec<String> {
    contract
        .components
        .iter()
        .filter(|component| component.language == LanguageKind::Cpp)
        .flat_map(|component| component.build.pkg_config.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn cmake_pkg_config_dependency_block(packages: &[String]) -> String {
    let mut output = String::from("\nfind_package(PkgConfig REQUIRED)\n");
    for (index, package) in packages.iter().enumerate() {
        output.push_str(&format!(
            "pkg_check_modules({} REQUIRED IMPORTED_TARGET {})\n",
            cmake_pkg_config_module_name(index, package),
            package
        ));
    }
    output
}

fn cmake_pkg_config_targets(packages: &[String]) -> Vec<String> {
    packages
        .iter()
        .enumerate()
        .map(|(index, package)| {
            format!(
                "PkgConfig::{}",
                cmake_pkg_config_module_name(index, package)
            )
        })
        .collect()
}

fn cmake_pkg_config_module_name(index: usize, package: &str) -> String {
    format!(
        "FLOWRT_PKG_{index}_{}",
        sanitize_cmake_identifier(package).to_ascii_uppercase()
    )
}

fn sanitize_cmake_identifier(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn cmake_iox2_dependency_block() -> &'static str {
    r#"
find_package(iceoryx2-cxx 0.9.1 QUIET)
if(NOT TARGET iceoryx2-cxx::static-lib-cxx)
  message(FATAL_ERROR "iceoryx2-cxx 0.9.1 was not found. Install the FlowRT package or set FLOWRT_CPP_RUNTIME_DIR/CMAKE_PREFIX_PATH to a FlowRT private prefix.")
endif()
"#
}

fn cmake_zenoh_discovery_block() -> &'static str {
    r#"
set(FLOWRT_ZENOH_CXX_TARGET "")
find_package(zenohc 1.9.0 QUIET)
find_package(zenohcxx 1.9.0 QUIET)
if(TARGET zenohcxx::zenohc)
  set(FLOWRT_ZENOH_CXX_TARGET zenohcxx::zenohc)
endif()
"#
}

fn cmake_required_zenoh_dependency_block() -> &'static str {
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

fn cmake_optional_zenoh_dependency_block() -> &'static str {
    cmake_zenoh_discovery_block()
}

fn contract_has_ros2_bridge_type(contract: &ContractIr, ros2_type: &str) -> bool {
    contract.graphs.iter().any(|graph| {
        graph
            .ros2_bridges
            .iter()
            .any(|bridge| bridge.ros2_type == ros2_type)
    })
}

fn cmake_ros2_bridge_dependency_block(has_pose_bridge: bool) -> String {
    let mut output = String::from(
        r#"
if(DEFINED ENV{AMENT_PREFIX_PATH})
  cmake_path(CONVERT "$ENV{AMENT_PREFIX_PATH}" TO_CMAKE_PATH_LIST FLOWRT_AMENT_PREFIX_PATH)
  list(PREPEND CMAKE_PREFIX_PATH ${FLOWRT_AMENT_PREFIX_PATH})
endif()
find_package(rclcpp REQUIRED)
"#,
    );
    if has_pose_bridge {
        output.push_str("find_package(geometry_msgs REQUIRED)\n");
    }
    output.push_str(
        r#"find_package(std_msgs REQUIRED)
find_package(rosidl_typesupport_cpp REQUIRED)
find_package(rmw_zenoh_cpp REQUIRED)
find_package(zenoh_cpp_vendor REQUIRED)
get_filename_component(FLOWRT_ROS2_ZENOH_VENDOR_PREFIX "${zenoh_cpp_vendor_DIR}/../../../opt/zenoh_cpp_vendor" ABSOLUTE)
set(FLOWRT_ROS2_ZENOH_INCLUDE "${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}/include")
set(FLOWRT_ROS2_ZENOH_LIB "${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}/lib/libzenohc.so")
if(NOT EXISTS "${FLOWRT_ROS2_ZENOH_INCLUDE}/zenoh.hxx" OR NOT EXISTS "${FLOWRT_ROS2_ZENOH_LIB}")
  message(FATAL_ERROR "rmw_zenoh_cpp must provide zenoh_cpp_vendor headers and libzenohc.so under ${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}. Install the ROS2 zenoh RMW package for the selected ROS2 distribution.")
endif()
"#,
    );
    output
}

fn cmake_ros2_bridge_target_block(bridge_target: &str, has_pose_bridge: bool) -> String {
    let geometry_msgs_target = if has_pose_bridge {
        " geometry_msgs::geometry_msgs__rosidl_typesupport_cpp"
    } else {
        ""
    };
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
target_link_libraries({bridge_target} PRIVATE rclcpp::rclcpp{geometry_msgs_target} std_msgs::std_msgs__rosidl_typesupport_cpp rosidl_typesupport_cpp::rosidl_typesupport_cpp ${{FLOWRT_ROS2_ZENOH_LIB}})
if(FLOWRT_EXE_LINK_OPTIONS)
  target_link_options({bridge_target} PRIVATE ${{FLOWRT_EXE_LINK_OPTIONS}})
endif()
if(FLOWRT_EXE_LINK_LIBRARIES)
  target_link_libraries({bridge_target} PRIVATE ${{FLOWRT_EXE_LINK_LIBRARIES}})
endif()
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
  target_link_options({bridge_target} PRIVATE "-Wl,--disable-new-dtags")
endif()
get_target_property(FLOWRT_ROS2_BRIDGE_BUILD_RPATH {bridge_target} BUILD_RPATH)
if(NOT FLOWRT_ROS2_BRIDGE_BUILD_RPATH)
  set(FLOWRT_ROS2_BRIDGE_BUILD_RPATH "")
endif()
set_property(TARGET {bridge_target} PROPERTY BUILD_RPATH "${{FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}}/lib;${{FLOWRT_ROS2_BRIDGE_BUILD_RPATH}}")
"#
    )
}
