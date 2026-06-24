# 通用发布元数据检查。由 scripts/check-release-readiness.sh source，并复用其 helper。

check_final_release_metadata_readiness() {
    printf '\n[29/32] README.md 安装示例\n'

    local readme_file="$repo_root/README.md"
    if [[ -f "$readme_file" ]]; then
        local readme_version
        local readme_match
        readme_version="$(first_match '(?<=^version=v)[0-9]+\.[0-9]+\.[0-9]+' "$readme_file")"
        if [[ -z "$readme_version" ]]; then
            readme_version="$(first_match '(?<=^version=)[0-9]+\.[0-9]+\.[0-9]+' "$readme_file")"
        fi
        readme_match="$(first_match 'flowrt_[0-9]+\.[0-9]+\.[0-9]+_amd64\.deb' "$readme_file")"
        if [[ -z "$readme_version" && -n "$readme_match" ]]; then
            readme_version="$(grep -oP '[0-9]+\.[0-9]+\.[0-9]+' <<<"$readme_match" | head -1)"
        fi
        if [[ "$readme_version" == "$expected_version" ]]; then
            pass "README.md 安装示例版本 = $readme_version"
        elif [[ -z "$readme_version" ]]; then
            info "README.md 中未找到版本化的 deb 文件名（可能是正常模板）"
        else
            fail "README.md 安装示例版本 = $readme_version，期望 $expected_version"
        fi
    fi

    printf '\n[30/32] CONTEXT.md 当前状态版本\n'

    local context_file="$repo_root/CONTEXT.md"
    if [[ -f "$context_file" ]]; then
        local context_version
        context_version="$(first_match '当前 workspace 版本(仍)?为 `\K[0-9]+\.[0-9]+\.[0-9]+' "$context_file")"
        if [[ "$context_version" == "$expected_version" ]]; then
            pass "CONTEXT.md 当前 workspace 版本 = $context_version"
        elif [[ -z "$context_version" ]]; then
            fail "CONTEXT.md 缺少 '当前 workspace 版本为 X.Y.Z' 状态行"
        else
            fail "CONTEXT.md 当前 workspace 版本 = $context_version，期望 $expected_version"
        fi
    else
        fail "CONTEXT.md 不存在"
    fi

    printf '\n[31/32] Release evidence 门禁覆盖\n'
    pass "release evidence 门禁由 v0.15.1 专项 adapter 覆盖"

    printf '\n[32/32] Git tag 检查\n'
    if git -C "$repo_root" tag -l "$expected_tag" | grep -q .; then
        info "tag $expected_tag 已存在"
    else
        info "tag $expected_tag 尚未创建（发布时由 CI 创建）"
    fi
}
