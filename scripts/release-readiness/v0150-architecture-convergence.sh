# v0.15.0 architecture convergence 的发布门禁接线检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0150_architecture_convergence_readiness() {
    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.15.0 focused gate"
    else
        require_ci_text "CI 包含 v0.15.0 architecture convergence smoke job" \
            "v0150-architecture-convergence-smoke:" "$ci_file"
        require_ci_text "v0.15.0 gate 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke 0.15.0" "$ci_file"
        require_ci_text_count_at_least "package/release/RC 依赖 v0.15.0 architecture gate" \
            "- v0150-architecture-convergence-smoke" "$ci_file" 3
    fi

    local registry_file="$repo_root/scripts/release-gates/registry.toml"
    if [[ -f "$registry_file" ]]; then
        local v0150_registry_entry
        v0150_registry_entry="$(
            awk '
                /^\[\[focused_smoke\]\]/ {
                    if (inside) {
                        exit;
                    }
                    block = "";
                    next;
                }
                {
                    block = block $0 "\n";
                    if ($0 == "version = \"0.15.0\"") {
                        inside = 1;
                    }
                }
                END {
                    if (inside) {
                        printf "%s", block;
                    }
                }
            ' "$registry_file"
        )"
        if grep -qF 'version = "0.15.0"' <<<"$v0150_registry_entry"; then
            pass "release gate registry 包含 v0.15.0 focused smoke"
        else
            fail "release gate registry 缺少 v0.15.0 focused smoke"
        fi
        if grep -qF "planned = true" <<<"$v0150_registry_entry"; then
            fail "v0.15.0 focused smoke 已落地，registry 不应继续标记 planned = true"
        else
            pass "v0.15.0 focused smoke 不再标记 planned"
        fi
    else
        fail "release gate registry 不存在: $registry_file"
    fi

    if release_gate_v0150_output="$(
        cargo run -q -p flowrt-devtools -- release-gate check-registry 0.15.0 2>&1
    )"; then
        pass "$release_gate_v0150_output"
    else
        fail "v0.15.0 release gate registry 校验失败: $release_gate_v0150_output"
    fi

    local installed_v0150_smoke="$repo_root/scripts/test-v0150-architecture-convergence-smoke.sh"
    if [[ -x "$installed_v0150_smoke" ]]; then
        pass "v0.15.0 architecture convergence smoke 脚本存在且可执行"
        require_file_text "v0.15.0 smoke 支持 dry run" \
            "FLOWRT_V0150_ARCHITECTURE_CONVERGENCE_SMOKE_DRY_RUN" "$installed_v0150_smoke"
        require_file_text "v0.15.0 smoke 运行 release gate registry 检查" \
            "release-gate check-registry 0.15.0" "$installed_v0150_smoke"
        require_file_text "v0.15.0 smoke 运行 architecture size guard" \
            "scripts/check-architecture-size.sh" "$installed_v0150_smoke"
        require_file_text "v0.15.0 smoke 保留 structure guard 入口" \
            "check_optional_structure_guard" "$installed_v0150_smoke"
    else
        fail "v0.15.0 architecture convergence smoke 脚本不存在或不可执行: $installed_v0150_smoke"
    fi
}
