#!/usr/bin/env bash
# 检查 generated/runtime/CLI 证据矩阵与 golden corpus、compile net 和 C++ 静态质量门禁一致。

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

matrix_file="scripts/evidence-matrix.toml"
corpus="crates/flowrt-codegen/tests/golden"

if [[ ! -f "$matrix_file" ]]; then
    printf 'evidence matrix not found: %s\n' "$matrix_file" >&2
    exit 1
fi

python3 - "$matrix_file" "$corpus" <<'PY'
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError as exc:
    raise SystemExit("python3 with tomllib is required to check evidence matrix") from exc

matrix_path = Path(sys.argv[1])
corpus = Path(sys.argv[2])
doc = tomllib.loads(matrix_path.read_text(encoding="utf-8"))
entries = doc.get("case", [])
if not entries:
    raise SystemExit("evidence matrix has no [[case]] entries")

allowed_languages = {"rust", "cpp", "c"}
allowed_surfaces = {
    "dataflow",
    "service",
    "operation",
    "params",
    "selfdesc",
    "record_replay",
    "pub_echo",
    "fault_matrix",
    "feedback",
    "sync",
    "lifecycle",
    "diagnostics",
    "variable_frame",
}
allowed_backends = {"inproc", "iox2", "zenoh", "auto_fallback"}
allowed_abi = {"fixed_plain", "bounded_frame", "unbounded_frame", "empty_message", "none"}
allowed_evidence = {
    "golden",
    "syntax_compile",
    "cargo_check",
    "runtime_smoke",
    "real_sdk_smoke",
    "cli_smoke",
    "static_quality",
}


def require_list(entry, field, allowed):
    value = entry.get(field)
    if not isinstance(value, list) or not value:
        raise SystemExit(f"matrix case `{entry.get('name', '<unknown>')}` field `{field}` must be a non-empty list")
    unknown = sorted(set(value) - allowed)
    if unknown:
        raise SystemExit(
            f"matrix case `{entry.get('name', '<unknown>')}` field `{field}` has unknown values: {', '.join(unknown)}"
        )
    return value


seen_names = set()
matrix_compile = set()
static_quality_cases = set()
surface_languages: dict[str, set[str]] = {}

for entry in entries:
    name = entry.get("name")
    if not isinstance(name, str) or not name:
        raise SystemExit("each matrix [[case]] needs a non-empty string `name`")
    if name in seen_names:
        raise SystemExit(f"duplicate evidence matrix case: {name}")
    seen_names.add(name)

    languages = require_list(entry, "languages", allowed_languages)
    surfaces = require_list(entry, "surfaces", allowed_surfaces)
    require_list(entry, "backends", allowed_backends)
    require_list(entry, "abi", allowed_abi)
    evidence = require_list(entry, "evidence", allowed_evidence)
    version = entry.get("required_by_release")
    if not isinstance(version, str) or not version:
        raise SystemExit(f"matrix case `{name}` needs required_by_release")

    case_dir = corpus / name
    if "golden" in evidence and not case_dir.is_dir():
        raise SystemExit(f"matrix case `{name}` claims golden evidence but golden case is missing")

    for surface in surfaces:
        surface_languages.setdefault(surface, set()).update(languages)

    if "syntax_compile" in evidence:
        for language in languages:
            shell_path = case_dir / "expected" / language / "src" / (
                "runtime_shell.rs" if language == "rust" else "runtime_shell.cpp"
            )
            if language == "c":
                continue
            if not shell_path.is_file():
                raise SystemExit(
                    f"matrix case `{name}` claims {language} syntax_compile but `{shell_path}` is missing"
                )
            matrix_compile.add((language, name))

    if entry.get("cpp_static_quality", False):
        if "cpp" not in languages:
            raise SystemExit(f"matrix case `{name}` enables cpp_static_quality without cpp language")
        if "syntax_compile" not in evidence:
            raise SystemExit(f"matrix case `{name}` enables cpp_static_quality without syntax_compile evidence")
        static_quality_cases.add(name)

golden_compile = set()
for case_dir in sorted(path for path in corpus.iterdir() if path.is_dir()):
    name = case_dir.name
    if (case_dir / "expected" / "rust" / "src" / "runtime_shell.rs").is_file():
        golden_compile.add(("rust", name))
    if (case_dir / "expected" / "cpp" / "src" / "runtime_shell.cpp").is_file():
        golden_compile.add(("cpp", name))

missing = sorted(golden_compile - matrix_compile)
extra = sorted(matrix_compile - golden_compile)
if missing or extra:
    if missing:
        print("missing codegen compile golden cases in evidence matrix:", file=sys.stderr)
        for language, name in missing:
            print(f"{language} {name}", file=sys.stderr)
    if extra:
        print("stale codegen compile cases in evidence matrix:", file=sys.stderr)
        for language, name in extra:
            print(f"{language} {name}", file=sys.stderr)
    raise SystemExit(1)

for surface in ("dataflow", "feedback", "service", "operation", "selfdesc", "variable_frame"):
    languages = surface_languages.get(surface, set())
    if not {"rust", "cpp"}.issubset(languages):
        raise SystemExit(
            f"surface `{surface}` needs both Rust and C++ evidence, got: {', '.join(sorted(languages)) or '<none>'}"
        )

static_required_surfaces = {"dataflow", "feedback", "service", "operation", "variable_frame"}
static_seen_surfaces = set()
for entry in entries:
    if entry.get("cpp_static_quality", False):
        static_seen_surfaces.update(entry["surfaces"])
missing_static = sorted(static_required_surfaces - static_seen_surfaces)
if missing_static:
    raise SystemExit(
        "cpp static quality representative cases miss surfaces: " + ", ".join(missing_static)
    )

print("evidence matrix covers generated runtime shell compile cases and representative C++ static quality surfaces")
PY
