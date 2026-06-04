# FlowRT 文档

本目录保存应随代码入库的配套文档。这里的文档面向用户和维护者，回答“怎么使用、怎么验证、当前示例覆盖什么、维护时遵守什么规则”。

## 入库文档

- [快速开始](getting-started.md)：安装 `flowrt`、检查 RSDL、生成产物、运行 Rust/C++ 示例。
- [CLI 参考](cli.md)：`check`、`prepare`、`build`、`run`、`launch`、`inspect` 的用途、参数和边界。
- [示例矩阵](examples.md)：每个 `examples/` 项目的 runtime、backend、推荐命令和外部依赖。
- [开发维护](development.md)：本仓库开发验证、文档维护、提交和生成物边界。

## 不入库文档

架构计划和规格草案用于本地设计推演，默认不纳入 Git。当前 `.gitignore` 明确排除这些文件：

- `docs/architecture-plan.md`
- `docs/rsdl-v0.1.md`
- `docs/contract-ir-v0.1.md`
- `docs/message-abi-v0.1.md`
- `docs/backend-contract.md`
- `docs/project-layout.md`

如果一份文档是在说明已落地的用户流程、命令、示例或维护规则，应放在本目录并入库；如果一份文档仍是语义设计、规格草案或未冻结架构计划，应保持本地文件，不加入索引。
