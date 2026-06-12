# ROS2 到 FlowRT 迁移盘点清单

## 迁移前盘点

| ROS2 事实 | 需要记录 |
|---|---|
| package / executable | 包名、可执行名、启动入口、依赖库 |
| node | node 名、namespace、生命周期、线程或 executor 假设 |
| subscription | topic、消息类型、QoS、触发的回调、是否需要 latest 或 FIFO |
| publisher | topic、消息类型、发布时机、频率、是否用于行为对比 |
| service / action | request/response 或长任务语义，是否可先用 FlowRT Service/Operation 表达 |
| parameter | 名称、类型、默认值、启动期或运行期修改、apply 时机 |
| timer | 周期、deadline、和订阅回调的依赖关系 |
| launch | 启动顺序、环境变量、进程依赖、restart 期望 |
| side effect | 串口、相机、NPU、Web/UDP、文件、共享内存、外部 SDK |
| test oracle | rosbag、live topic、固定输入 JSON/JSONL、期望输出、误差范围 |

## 映射规则

| ROS2 概念 | FlowRT 迁移映射 |
|---|---|
| node | 通常拆成 component type、instance、tasks 和 process；不要机械一比一 |
| topic 输入 | 已迁移来源用 `[[bind]]`；未迁移来源用 `boundary.input` |
| topic 输出 | 已迁移下游用 `[[bind]]`；对比或外部输出用 `boundary.output` |
| callback | 根据触发方式建 task：`on_message`、`periodic`、`startup` 或 `shutdown` |
| QoS depth | 映射为 channel policy：`latest(depth=1)` 或 `fifo(depth=N)` |
| parameter | 映射为 FlowRT 参数 schema、默认值和 instance override |
| launch | 映射为 process orchestration、env、readiness 和 restart policy |
| 硬件/SDK | 优先建 I/O boundary 或 external process 候选，不写进纯算法 component |

## 行为对比步骤

1. 选一个最小功能单位，写 `mode = "island"`。
2. 为所有未迁移输入声明 typed `boundary.input`。
3. 为要对比的输出声明 typed `boundary.output`。
4. 运行 `flowrt deps`、`flowrt build`、`flowrt run`。
5. 在 FlowRT 外部把 bag、live topic 或旧测试 fixture 转换为 RSDL 字段自然 JSONL
   或 JSON array；当前不让 FlowRT 原生读取 ROS2 bag。
6. 用 `flowrt params set --file params.json --image ...` 应用迁移测试参数。
7. 用 `flowrt pub <boundary> --file samples.jsonl --freq <hz> --image ...` 或 ROS2 bridge
   输入同一批样本。
8. 用 `flowrt echo out_a out_b --image ...`、`flowrt record --channel <boundary-output>`
   或 test sink 捕获输出。
9. 对比字段值、时间戳、频率、stale/overflow 和错误状态。
10. 每迁完一个相邻功能单位，删除对应 boundary，改用普通 FlowRT bind。
11. 全部 boundary 删除后，把 profile 切回 `strict`。

## 完成定义

- RSDL 中没有未解释的残缺拓扑。
- 所有 boundary 都有明确的拆除计划，或已经被普通 bind 替换。
- 行为对比有命令、输入样本、输出结果和误差说明。
- 如果使用外部 ROS2 数据，已记录转换命令或脚本，并说明 FlowRT 输入 JSONL 与 RSDL
  类型字段一一对应。
- 用户算法只在 `src/`；`flowrt/` 生成物可删除重建。
- 需要长期保留的 I/O 或外部进程有显式边界，不混入纯 dataflow component。
