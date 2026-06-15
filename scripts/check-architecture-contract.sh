#!/usr/bin/env bash
# 检查 v0.15.0 架构收敛的结构合同，确认核心事实模块已进入生产消费路径。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

pass() {
    printf '  ✓ %s\n' "$1"
}

info() {
    printf '→ %s\n' "$1"
}

fail() {
    printf '错误: %s\n' "$1" >&2
    exit 1
}

require_tool() {
    local tool="$1"

    if command -v "$tool" >/dev/null 2>&1; then
        pass "工具可用：$tool"
    else
        fail "缺少必要工具：$tool"
    fi
}

require_command() {
    local label="$1"
    shift

    local output
    if output="$("$@" 2>&1)"; then
        pass "$label"
        if [[ -n "$output" ]]; then
            printf '%s\n' "$output"
        fi
    else
        printf '%s\n' "$output" >&2
        fail "$label 失败"
    fi
}

require_command_output() {
    local label="$1"
    local expected="$2"
    shift 2

    local output
    if output="$("$@" 2>&1)"; then
        if [[ "$output" == "$expected" ]]; then
            pass "$label"
        else
            fail "$label 输出不匹配：期望 '$expected'，实际 '$output'"
        fi
    else
        printf '%s\n' "$output" >&2
        fail "$label 失败"
    fi
}

require_file_text() {
    local label="$1"
    local needle="$2"
    local file="$3"

    if [[ ! -f "$file" ]]; then
        fail "$label: 文件不存在：$file"
    fi
    if rg --fixed-strings --quiet -- "$needle" "$file"; then
        pass "$label"
    else
        fail "$label: $file 缺少精确文本 '$needle'"
    fi
}

registry_entry_for_version() {
    local version="$1"
    local registry_file="$2"

    awk -v wanted="version = \"${version}\"" '
        /^\[\[focused_smoke\]\]/ {
            if (inside) {
                exit;
            }
            block = "";
            next;
        }
        {
            block = block $0 "\n";
            if ($0 == wanted) {
                inside = 1;
            }
        }
        END {
            if (inside) {
                printf "%s", block;
            }
        }
    ' "$registry_file"
}

check_release_gate_contract() {
    local registry_file="scripts/release-gates/registry.toml"
    local expected_smoke="scripts/test-v0150-architecture-convergence-smoke.sh"

    info "release gate contract"
    require_command \
        "release gate registry 可由 devtools 校验" \
        cargo run -q -p flowrt-devtools -- release-gate check-registry 0.15.0
    require_command_output \
        "v0.15.0 focused smoke 指向 architecture convergence smoke" \
        "$expected_smoke" \
        cargo run -q -p flowrt-devtools -- release-gate focused-smoke 0.15.0

    local registry_entry
    registry_entry="$(registry_entry_for_version "0.15.0" "$registry_file")"
    if [[ -z "$registry_entry" ]]; then
        fail "release gate registry 缺少 v0.15.0 focused smoke 条目"
    fi
    if rg --fixed-strings --quiet -- "planned = true" <<<"$registry_entry"; then
        fail "v0.15.0 focused smoke 已落地，registry 不得继续 planned = true"
    fi
    pass "v0.15.0 focused smoke registry 条目不是 planned"
}

check_contract_ir_derived_facts() {
    info "Contract IR derived facts Module"
    require_file_text \
        "flowrt-ir 暴露 derived module" \
        "pub mod derived;" \
        "crates/flowrt-ir/src/lib.rs"
    require_file_text \
        "derived/mod.rs 暴露 ContractDerivedFacts" \
        "pub struct ContractDerivedFacts" \
        "crates/flowrt-ir/src/derived/mod.rs"
    require_file_text \
        "derived/mod.rs 暴露 derive_contract_facts" \
        "pub fn derive_contract_facts" \
        "crates/flowrt-ir/src/derived/mod.rs"

    require_file_text \
        "validator capabilities 直接消费 ContractDerivedFacts" \
        "derived::{ContractDerivedFacts, GraphDerivedFacts, derive_contract_facts}" \
        "crates/flowrt-validate/src/capabilities.rs"
    require_file_text \
        "validator capabilities 重新推导 derived facts" \
        "derive_contract_facts(ir)" \
        "crates/flowrt-validate/src/capabilities.rs"
    require_file_text \
        "validator resources 直接消费 derive_contract_facts" \
        "derived::derive_contract_facts" \
        "crates/flowrt-validate/src/resources.rs"
    require_file_text \
        "validator resources 使用 derived facts 校验 satisfactions" \
        "derive_contract_facts(ir).ok().map(|facts|" \
        "crates/flowrt-validate/src/resources.rs"
    require_file_text \
        "codegen runtime plan 直接消费 ContractDerivedFacts" \
        "derived::{ContractDerivedFacts, GraphDerivedFacts, derive_contract_facts}" \
        "crates/flowrt-codegen/src/runtime_plan.rs"
    require_file_text \
        "codegen runtime plan 提供 contract_derived_facts adapter" \
        "pub(crate) fn contract_derived_facts" \
        "crates/flowrt-codegen/src/runtime_plan.rs"
    require_file_text \
        "launch manifest 消费 GraphDerivedFacts" \
        "derived::GraphDerivedFacts" \
        "crates/flowrt-codegen/src/launch_manifest.rs"
    require_file_text \
        "launch manifest 通过 adapter 读取 derived facts" \
        "let facts = contract_derived_facts(contract)?;" \
        "crates/flowrt-codegen/src/launch_manifest.rs"
    require_file_text \
        "self-description 消费 GraphDerivedFacts" \
        "derived::GraphDerivedFacts" \
        "crates/flowrt-codegen/src/selfdesc.rs"
    require_file_text \
        "self-description 通过 adapter 读取 derived facts" \
        "let facts = contract_derived_facts(contract)?;" \
        "crates/flowrt-codegen/src/selfdesc.rs"
}

check_runtime_observability_facts() {
    info "runtime observability facts Module"
    require_file_text \
        "facts.rs 定义 RuntimeObservabilityFacts" \
        "struct RuntimeObservabilityFacts" \
        "runtime/rust/src/introspection/facts.rs"
    require_file_text \
        "facts.rs 从 state inner 派生观测事实" \
        "fn from_state_inner" \
        "runtime/rust/src/introspection/facts.rs"
    require_file_text \
        "state.rs 消费 RuntimeObservabilityFacts" \
        "use super::facts::{RuntimeObservabilityFacts, input_status_key};" \
        "runtime/rust/src/introspection/state.rs"
    require_file_text \
        "state.rs status 快照来自 RuntimeObservabilityFacts" \
        "RuntimeObservabilityFacts::from_state_inner(&inner, recorder).status_snapshot()" \
        "runtime/rust/src/introspection/state.rs"
    require_file_text \
        "state.rs recorder diagnostics 消费观测事实" \
        "fn record_diagnostics_events(&self, facts: &RuntimeObservabilityFacts)" \
        "runtime/rust/src/introspection/state.rs"
    require_file_text \
        "diagnostics.rs 通过 facts module 派生 diagnostics" \
        "facts::derive_diagnostic_facts(status)" \
        "runtime/rust/src/introspection/diagnostics.rs"
    require_file_text \
        "recorder.rs 消费 RecorderDiagnosticFact" \
        "use crate::introspection::facts::RecorderDiagnosticFact;" \
        "runtime/rust/src/recorder.rs"
    require_file_text \
        "recorder.rs 接收 facts module 的 diagnostics fact" \
        "fact: &RecorderDiagnosticFact" \
        "runtime/rust/src/recorder.rs"
}

cd "$repo_root"

printf 'architecture contract 检查开始：repo=%s\n' "$repo_root"
require_tool rg
check_release_gate_contract
check_contract_ir_derived_facts
check_runtime_observability_facts
printf 'architecture contract 检查通过：release gate、Contract IR derived facts、runtime observability facts 均已进入生产消费路径。\n'
