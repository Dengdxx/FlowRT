# v0.14.1 architecture guard 的发布就绪检查。
# 本文件由 scripts/check-release-readiness.sh source，并复用其 pass/fail helper。

check_v0141_architecture_readiness() {
    if [[ ! -f "$ci_file" ]]; then
        fail "CI 配置不存在，无法检查 v0.14.1 focused gate"
    else
        require_ci_text "CI 包含 v0.14.1 architecture smoke job" \
            "v0141-architecture-smoke:" "$ci_file"
        require_ci_text "v0.14.1 gate 运行 architecture smoke 脚本" \
            "scripts/test-v0141-architecture-smoke.sh" "$ci_file"
        require_ci_text "v0.14.1 gate 运行 architecture size guard" \
            "scripts/check-architecture-size.sh" "$ci_file"
        require_ci_text_count_at_least "package/release evidence 依赖 v0.14.1 architecture gate" \
            "- v0141-architecture-smoke" "$ci_file" 2
    fi

    local architecture_size_script="$repo_root/scripts/check-architecture-size.sh"
    if [[ -x "$architecture_size_script" ]]; then
        pass "architecture size guard 脚本存在且可执行"
        require_file_text "architecture guard 默认 Rust 阈值 2500 行" \
            'FLOWRT_ARCH_SIZE_RUST_LIMIT:-2500' "$architecture_size_script"
        require_file_text "architecture guard 默认 C/C++ 阈值 2500 行" \
            'FLOWRT_ARCH_SIZE_CPP_LIMIT:-2500' "$architecture_size_script"
        require_file_text "architecture guard 默认 shell 阈值 1200 行" \
            'FLOWRT_ARCH_SIZE_SHELL_LIMIT:-1200' "$architecture_size_script"
        if grep -qF "add_legacy_file" "$architecture_size_script"; then
            fail "architecture guard 仍保留 legacy allowlist 入口"
        else
            pass "architecture guard 不保留 legacy allowlist"
        fi
    else
        fail "architecture size guard 脚本不存在或不可执行: $architecture_size_script"
    fi

    local installed_v0141_smoke="$repo_root/scripts/test-v0141-architecture-smoke.sh"
    if [[ -x "$installed_v0141_smoke" ]]; then
        pass "v0.14.1 architecture smoke 脚本存在且可执行"
        require_file_text "v0.14.1 smoke 支持 dry run" \
            "FLOWRT_V0141_ARCHITECTURE_SMOKE_DRY_RUN" "$installed_v0141_smoke"
        require_file_text "v0.14.1 smoke 运行脚本语法检查" \
            "bash -n" "$installed_v0141_smoke"
        require_file_text "v0.14.1 smoke 运行 architecture size guard" \
            "scripts/check-architecture-size.sh" "$installed_v0141_smoke"
    else
        fail "v0.14.1 architecture smoke 脚本不存在或不可执行: $installed_v0141_smoke"
    fi

    local release_candidate_script="$repo_root/scripts/check-release-candidate.sh"
    if [[ -x "$release_candidate_script" ]]; then
        require_file_text "release evidence helper 通过 devtools registry 查询 focused smoke" \
            "release-gate focused-smoke" "$release_candidate_script"
    else
        fail "release evidence helper 不存在或不可执行: $release_candidate_script"
    fi

    local v0141_release_body
    v0141_release_body="$(
        awk '
            /^## v0\.14\.1 - / {
                inside = 1;
                next;
            }
            inside && /^## / {
                exit;
            }
            inside {
                print;
            }
        ' "$repo_root/CHANGELOG.md"
    )"

    local v0141_notes_source="CHANGELOG v0.14.1 版本段"
    local v0141_notes_body="$v0141_release_body"
    if [[ -z "$v0141_notes_body" ]]; then
        v0141_notes_source="CHANGELOG 未发布段"
        v0141_notes_body="$(
            awk '
                /^## 未发布$/ {
                    inside = 1;
                    next;
                }
                inside && /^## / {
                    exit;
                }
                inside {
                    print;
                }
            ' "$repo_root/CHANGELOG.md"
        )"
    fi

    if [[ -z "$v0141_notes_body" ]]; then
        fail "CHANGELOG.md 缺少可作为 v0.14.1 release notes 事实源的版本段或未发布段"
    else
        if grep -qF '0.14.1' <<<"$v0141_notes_body"; then
            pass "$v0141_notes_source 记录 0.14.1 定位"
        else
            fail "$v0141_notes_source 缺少 0.14.1 定位条目"
        fi
        if grep -qF 'SchedulerRuntimePlan' <<<"$v0141_notes_body"; then
            pass "$v0141_notes_source 记录 SchedulerRuntimePlan"
        else
            fail "$v0141_notes_source 缺少 SchedulerRuntimePlan 条目"
        fi
        if grep -qF '大文件拆分' <<<"$v0141_notes_body"; then
            pass "$v0141_notes_source 记录大文件拆分"
        else
            fail "$v0141_notes_source 缺少大文件拆分条目"
        fi
        if grep -qF 'architecture guard' <<<"$v0141_notes_body"; then
            pass "$v0141_notes_source 记录 architecture guard"
        else
            fail "$v0141_notes_source 缺少 architecture guard 条目"
        fi
    fi
}
