---
name: frt-app-ros2-migration
description: >
  Use when migrating ROS2 packages, nodes, topics, params, launch files, or
  behavior tests into a FlowRT app, especially when using Island Mode,
  boundary endpoints, flowrt pub, record, echo, or ROS2 coexistence bridges.
---

# FlowRT ROS2 迁移

## Overview

这个 skill 防止 ROS2 项目迁移时把 FlowRT 降级成“另一个 ROS2 壳”。迁移主路径是：
先用 `island` profile 和 typed boundary endpoint 做可拆 IO 脚手架，完成行为对比后
逐步替换为普通 FlowRT bind，最后回到 `strict` 生产 graph。

## When to Use

- 用户说“把 ROS2 包 / node / topic / launch 迁到 FlowRT”。
- 用户需要先迁一个功能单位，并用原 ROS2 输入输出做行为对比。
- 用户要使用 `flowrt pub`、`flowrt echo`、`flowrt record` 或 ROS2 bridge 验证 app。

不要在以下场景使用：

- 修改 FlowRT core 语义、RSDL、IR、validator、codegen 或 runtime。
- 从零开发普通 FlowRT app，且没有 ROS2 迁移或对比需求。
- 试图做 ROS2 drop-in 兼容层。

<HARD-GATE>
迁移前必须确认：

- `strict` 模式下残缺拓扑仍是错误；只有显式 `mode = "island"` 才能用 boundary 补齐 IO。
- 每个 boundary endpoint 都必须 typed，并绑定真实 `instance.port`，不能写成 backend、
  ROS2 topic、SDK handle 或临时调试口。
- 先定义行为 oracle：rosbag、live ROS2 topic、固定 JSON 样本、MCAP 或测试 fixture。
  没有对比输入输出，不得宣布迁移完成。
- 用户算法只能进 `src/` 用户代码；不得手写或修改 `flowrt/` 生成物。
- 串口、相机、NPU、Web/UDP 等副作用必须标为 I/O boundary 或 external process 候选，
  不能伪装成纯 dataflow component。
- island 产物是脚手架；迁移完成必须删除 boundary endpoint，并把 profile 切回 `strict`。
</HARD-GATE>

## Process

1. **盘点 ROS2 事实**：列出 package、node、topic、service/action、parameter、timer、
   launch、硬件 IO、副作用和测试数据。使用 `references/ros2-migration-checklist.md`。
2. **选择 migration island**：一次只迁一个功能单位。ROS2 node 通常映射为 FlowRT
   component instance + tasks + process，不要机械一比一照搬。
3. **建立 RSDL island**：外部输入写 `boundary.input`，待对比输出写 `boundary.output`；
   能闭合的 FlowRT 内部连接写普通 `[[bind]]`。
4. **迁移算法代码**：把纯算法放进 FlowRT component；把 I/O 和副作用留在明确边界。
5. **跑行为对比**：用 `flowrt pub` 或 ROS2 bridge 喂 boundary input，用 `echo` /
   `record` / test sink 观察 boundary output，并记录命令和关键输出。
6. **拆脚手架**：每迁完相邻功能单位，就用普通 bind 替换对应 boundary；全部闭合后切回
   `strict`。

## Common Rationalizations

| 借口 | 现实 |
|---|---|
| “先 warning 生成，后面再补拓扑” | FlowRT 生产默认是 strict；残缺拓扑只能在 island 下显式表达 |
| “ROS2 node 就等于 FlowRT component” | node 是 ROS2 容器；FlowRT 要拆成 component、instance、task 和 process |
| “先把 topic 名搬过来” | FlowRT channel/boundary 必须 typed，并绑定真实端口 |
| “跑起来就算迁完” | 没有输入输出行为对比，就不知道是否迁对 |
| “bridge 能用就不用拆 boundary” | bridge 是迁移边界，不是长期拓扑替代品 |

## Quick Reference

| 目标 | 主路径 |
|---|---|
| 单包迁移 | `mode = "island"` + typed boundary input/output |
| 手动喂输入 | `flowrt pub <boundary-input> --json ... --image ...` |
| 观察输出 | `flowrt echo <boundary-output> --image ...` |
| 录制对比 | `flowrt record --channel <boundary-output>` |
| 临时 ROS2 共存 | ROS2 bridge 绑定 boundary endpoint，仍保持 zenoh 隔离 |
| 迁移完成 | 删除 boundary，改普通 bind，profile 切回 `strict` |
