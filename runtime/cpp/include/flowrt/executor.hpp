#pragma once

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <coroutine>
#include <cstdint>
#include <deque>
#include <exception>
#include <flowrt/core.hpp>
#include <functional>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <queue>
#include <set>
#include <thread>
#include <type_traits>
#include <vector>

namespace flowrt {

struct TaskId {
    std::uint64_t value{};

    friend constexpr bool operator==(TaskId, TaskId) = default;
    friend constexpr auto operator<=>(TaskId, TaskId) = default;
};

struct LaneId {
    std::uint64_t value{};

    friend constexpr bool operator==(LaneId, LaneId) = default;
    friend constexpr auto operator<=>(LaneId, LaneId) = default;
};

enum class LaneKind {
    Serial,
    Parallel,
};

struct TaskSpec {
    TaskId id;
    LaneId lane;
    std::uint32_t priority{};
};

struct PeriodicSpec {
    TaskId task;
    std::chrono::milliseconds period{1};
};

/**
 * @brief 外部 global tick 协调器授予单进程 runtime shell 的一步执行许可。
 */
struct ExternalTick {
    std::uint64_t tick_id{0};          ///< 全局 tick 序号，由 coordinator 分配。
    std::uint64_t logical_time_ms{0};  ///< 本 tick 对应的逻辑毫秒时间。
};

/**
 * @brief runtime shell 完成外部 tick 后回报给 coordinator 的结果。
 */
struct ExternalTickReport {
    std::uint64_t tick_id{0};   ///< 完成的 tick 序号。
    Status status{Status::Ok};  ///< 本 tick 的 FlowRT 执行状态。

    static constexpr ExternalTickReport make(std::uint64_t tick_id, Status status) noexcept {
        return ExternalTickReport{.tick_id = tick_id, .status = status};
    }

    static constexpr ExternalTickReport ok(std::uint64_t tick_id) noexcept {
        return make(tick_id, Status::Ok);
    }
};

/**
 * @brief 一次 task admission 的调度元数据。
 *
 * generated shell 用这些字段构造 `TaskTiming`，并在 worker completion 回来后调用
 * `DeterministicExecutor::complete_task` 释放 inflight token。
 */
struct TaskAdmission {
    TaskId task;
    LaneId lane;
    std::uint64_t scheduled_time_ms{};
    std::uint64_t observed_time_ms{};
    std::optional<std::uint64_t> period_ms;
    std::optional<std::uint64_t> deadline_ms;
    std::uint64_t missed_periods{};
    std::uint64_t lateness_ms{};

    friend constexpr bool operator==(const TaskAdmission &, const TaskAdmission &) = default;
};

struct TaskRunResult {
    TaskId task;
    Status status{Status::Ok};

    friend constexpr bool operator==(const TaskRunResult &, const TaskRunResult &) = default;
};

/**
 * @brief 单个 task 回调返回的状态和 task-local 输出集合。
 *
 * @tparam Outputs generated shell 收集的输出集合类型。
 */
template <typename Outputs>
struct TaskRunOutcome {
    using outputs_type = Outputs;

    Status status{Status::Ok};  ///< 用户回调返回的 FlowRT 状态。
    Outputs outputs;            ///< 回调结束后由 generated shell 收集的输出集合。

    /**
     * @brief 构造成功结果；scheduler 后续可以提交其中输出。
     */
    static TaskRunOutcome ok(Outputs outputs) {
        return TaskRunOutcome{.status = Status::Ok, .outputs = std::move(outputs)};
    }

    /**
     * @brief 构造重试结果；runtime 会丢弃其中输出。
     */
    static TaskRunOutcome retry(Outputs outputs) {
        return TaskRunOutcome{.status = Status::Retry, .outputs = std::move(outputs)};
    }

    /**
     * @brief 构造错误结果；runtime 会丢弃其中输出。
     */
    static TaskRunOutcome error(Outputs outputs) {
        return TaskRunOutcome{.status = Status::Error, .outputs = std::move(outputs)};
    }
};

/**
 * @brief scheduler 线程可观察到的 task 执行结果。
 *
 * @tparam Outputs generated shell 收集的输出集合类型。
 *
 * `outputs` 只在 `status == Status::Ok` 时保留；非 `Ok` 或 exception 路径统一为空，
 * 防止 generated shell 误提交失败回调写入的样本。
 */
template <typename Outputs>
struct TaskRunOutput {
    TaskId task;                     ///< 被执行的 task id。
    Status status{Status::Ok};       ///< task 执行状态。
    std::optional<Outputs> outputs;  ///< 可提交输出集合；只有成功 task 会保留。

    /**
     * @brief 从用户回调正常返回的结果构造 scheduler 可见结果。
     */
    static TaskRunOutput from_outcome(TaskId task, TaskRunOutcome<Outputs> outcome) {
        auto status = outcome.status;
        std::optional<Outputs> outputs;
        if (status == Status::Ok) {
            outputs = std::move(outcome.outputs);
        }
        return TaskRunOutput{.task = task, .status = status, .outputs = std::move(outputs)};
    }

    /**
     * @brief 构造不携带输出的结果，用于非 `Ok` 或 exception 路径。
     */
    static TaskRunOutput without_outputs(TaskId task, Status status) {
        return TaskRunOutput{.task = task, .status = status, .outputs = std::nullopt};
    }
};

/**
 * @brief worker 完成后由 scheduler 线程回收的 typed completion queue。
 *
 * `drain_completed` 与 `try_drain_completed` 只回收当前已完成结果，空队列立即返回。
 */
template <typename Outputs>
class WorkerCompletionQueue {
   public:
    /**
     * @brief 写入一条已完成 task 结果。
     */
    void push(TaskRunOutput<Outputs> result) const {
        std::function<void()> wake;
        {
            std::lock_guard lock(state_->mutex);
            state_->completed.push_back(std::move(result));
            wake = state_->wake;
        }
        if (wake) {
            wake();
        }
    }

    /**
     * @brief 非阻塞回收当前所有已完成结果。
     */
    [[nodiscard]] std::vector<TaskRunOutput<Outputs>> drain_completed() const {
        std::lock_guard lock(state_->mutex);
        std::vector<TaskRunOutput<Outputs>> completed;
        completed.reserve(state_->completed.size());
        while (!state_->completed.empty()) {
            completed.push_back(std::move(state_->completed.front()));
            state_->completed.pop_front();
        }
        return completed;
    }

    /**
     * @brief 非阻塞尝试回收当前所有已完成结果。
     */
    [[nodiscard]] std::vector<TaskRunOutput<Outputs>> try_drain_completed() const {
        return drain_completed();
    }

    /**
     * @brief 设置 completion 入队后的唤醒回调。
     *
     * generated scheduler 用它把 worker completion 合并进 scheduler wake 路径；回调不拥有
     * completion 数据，只负责唤醒调度线程。
     */
    void set_wake_callback(std::function<void()> wake) const {
        std::lock_guard lock(state_->mutex);
        state_->wake = std::move(wake);
    }

   private:
    struct State {
        mutable std::mutex mutex;
        std::deque<TaskRunOutput<Outputs>> completed;
        std::function<void()> wake;
    };

    std::shared_ptr<State> state_{std::make_shared<State>()};
};

enum class ScheduleEvent {
    Data,
    Timer,
    Shutdown,
};

class ScheduleWaiter {
   public:
    ScheduleWaiter() : state_(std::make_shared<State>()) {}

    void notify_data() const { notify_data_with_time(std::nullopt); }

    void notify_data_at_ms(std::uint64_t time_ms) const { notify_data_with_time(time_ms); }

    [[nodiscard]] std::optional<std::uint64_t> take_data_time_ms() const {
        std::lock_guard lock(state_->mutex);
        auto value = state_->data_time_ms;
        state_->data_time_ms.reset();
        return value;
    }

    void notify_data_with_time(std::optional<std::uint64_t> time_ms) const {
        std::lock_guard lock(state_->mutex);
        ++state_->data_generation;
        if (time_ms.has_value()) {
            state_->data_time_ms = state_->data_time_ms.has_value()
                                       ? std::max(*state_->data_time_ms, *time_ms)
                                       : time_ms;
        }
        state_->ready.notify_all();
    }

    [[nodiscard]] std::uint64_t data_generation() const {
        std::lock_guard lock(state_->mutex);
        return state_->data_generation;
    }

    [[nodiscard]] ScheduleEvent wait_until(
        std::optional<std::chrono::steady_clock::time_point> deadline,
        const ShutdownToken &shutdown) {
        return wait_until_after(data_generation(), deadline, shutdown);
    }

    [[nodiscard]] ScheduleEvent wait_until_after(
        std::uint64_t seen_generation,
        std::optional<std::chrono::steady_clock::time_point> deadline,
        const ShutdownToken &shutdown) {
        std::unique_lock lock(state_->mutex);
        constexpr auto shutdown_poll = std::chrono::milliseconds{50};
        while (true) {
            if (shutdown.is_requested()) {
                return ScheduleEvent::Shutdown;
            }
            if (state_->data_generation != seen_generation) {
                return ScheduleEvent::Data;
            }
            if (deadline.has_value()) {
                const auto now = std::chrono::steady_clock::now();
                if (now >= *deadline) {
                    return ScheduleEvent::Timer;
                }
                const auto wake_deadline = std::min(*deadline, now + shutdown_poll);
                if (state_->ready.wait_until(lock, wake_deadline) == std::cv_status::timeout &&
                    std::chrono::steady_clock::now() >= *deadline) {
                    return ScheduleEvent::Timer;
                }
            } else {
                state_->ready.wait_for(lock, shutdown_poll);
            }
        }
    }

   private:
    struct State {
        std::mutex mutex;
        std::condition_variable ready;
        std::uint64_t data_generation{};
        std::optional<std::uint64_t> data_time_ms;
    };

    std::shared_ptr<State> state_;
};

class WorkerPool;

class ReadyBatch {
   public:
    ReadyBatch() = default;

    static ReadyBatch from_admissions(std::vector<TaskAdmission> admissions) {
        std::vector<TaskId> tasks;
        tasks.reserve(admissions.size());
        for (const auto admission : admissions) {
            tasks.push_back(admission.task);
        }
        return ReadyBatch{std::move(tasks), std::move(admissions)};
    }

    [[nodiscard]] bool empty() const noexcept { return tasks_.empty(); }

    [[nodiscard]] std::size_t size() const noexcept { return tasks_.size(); }

    [[nodiscard]] const std::vector<TaskId> &tasks() const noexcept { return tasks_; }

    /**
     * @brief 返回 admission metadata；顺序即 generated scheduler 的 canonical commit 顺序。
     */
    [[nodiscard]] const std::vector<TaskAdmission> &admissions() const noexcept {
        return admissions_;
    }

   private:
    ReadyBatch(std::vector<TaskId> tasks, std::vector<TaskAdmission> admissions)
        : tasks_(std::move(tasks)), admissions_(std::move(admissions)) {}

    std::vector<TaskId> tasks_;
    std::vector<TaskAdmission> admissions_;
};

class DeterministicExecutor {
   public:
    explicit DeterministicExecutor(std::size_t worker_threads)
        : worker_threads_(std::max<std::size_t>(1, worker_threads)) {}

    [[nodiscard]] std::size_t worker_threads() const noexcept { return worker_threads_; }

    void add_lane(LaneId lane, LaneKind kind) { lanes_[lane] = kind; }

    void add_task(TaskSpec spec) { tasks_[spec.id] = TaskState{.spec = spec}; }

    /**
     * @brief 设置 task 的相对 deadline 元数据，单位为毫秒。
     */
    void set_task_deadline_ms(TaskId task, std::optional<std::uint64_t> deadline_ms) {
        const auto it = tasks_.find(task);
        if (it != tasks_.end()) {
            it->second.deadline_ms = deadline_ms;
        }
    }

    void add_periodic(PeriodicSpec spec) {
        const auto period = std::max(spec.period, std::chrono::milliseconds{1});
        periodic_[spec.task] = PeriodicState{
            .period = period,
            .next_deadline = now_ + period,
            .missed_periods = 0,
        };
    }

    void wake(TaskId task) {
        if (admission_open_ && !suspended_.contains(task) && tasks_.contains(task)) {
            ready_.insert(task);
            if (!pending_.contains(task)) {
                pending_[task] = PendingAdmission{
                    .scheduled_time_ms = static_cast<std::uint64_t>(now_.count()),
                };
            }
        }
    }

    /**
     * @brief 故障隔离：暂停某 task 的调度。
     *
     * 把它移出 ready/pending，后续 `wake`/periodic due 也不再让它进入 ready，直到
     * `resume_task`。已 inflight 的提交不受影响，completion 仍会回来。
     */
    void suspend_task(TaskId task) {
        suspended_.insert(task);
        ready_.erase(task);
        pending_.erase(task);
    }

    /**
     * @brief 重启成功：恢复某 task 的调度资格。
     *
     * 下次 `wake` 或 periodic due 起重新进入 ready。
     */
    void resume_task(TaskId task) { suspended_.erase(task); }

    void close_admission() noexcept { admission_open_ = false; }

    [[nodiscard]] bool is_drained() const noexcept {
        return ready_.empty() && inflight_tasks_.empty();
    }

    void advance_to(std::chrono::milliseconds now) {
        now_ = now;
        struct PeriodicDue {
            TaskId task;
            std::chrono::milliseconds scheduled_time;
            std::chrono::milliseconds period;
            std::uint64_t missed{};
        };
        std::vector<PeriodicDue> due;
        for (auto &[task, state] : periodic_) {
            if (now_ < state.next_deadline) {
                continue;
            }
            const auto elapsed = now_ - state.next_deadline;
            const auto missed = static_cast<std::uint64_t>(elapsed / state.period);
            const auto scheduled_time =
                saturated_add(state.next_deadline, saturated_mul(state.period, missed));
            state.next_deadline = saturated_add(scheduled_time, state.period);
            due.push_back(PeriodicDue{
                .task = task,
                .scheduled_time = scheduled_time,
                .period = state.period,
                .missed = missed,
            });
        }
        for (const auto event : due) {
            record_periodic_due(event.task, event.scheduled_time, event.period, event.missed);
        }
    }

    [[nodiscard]] std::chrono::milliseconds next_deadline(TaskId task) const {
        const auto it = periodic_.find(task);
        if (it == periodic_.end()) {
            return std::chrono::milliseconds{0};
        }
        return it->second.next_deadline;
    }

    [[nodiscard]] std::uint64_t missed_periods(TaskId task) const {
        const auto it = periodic_.find(task);
        if (it == periodic_.end()) {
            return 0;
        }
        return it->second.missed_periods;
    }

    /**
     * @brief 准入当前 ready set 中可并行 dispatch 的 task。
     *
     * 该方法只修改 scheduler admission 状态，不运行用户回调，也不等待 completion。
     */
    [[nodiscard]] ReadyBatch take_ready_batch() {
        std::vector<TaskAdmission> admissions;
        for (const auto task : ready_parallel_order()) {
            auto admission = admit_task(task);
            if (admission.has_value()) {
                admissions.push_back(*admission);
            }
        }
        return ReadyBatch::from_admissions(std::move(admissions));
    }

    /**
     * @brief 标记 task completion，并释放它实际持有的 inflight token。
     *
     * 返回 `true` 表示本次释放成功；错误或重复 completion 返回 `false` 且不释放其他 lane。
     */
    bool complete_task(TaskId task) {
        if (inflight_tasks_.erase(task) == 0U) {
            return false;
        }
        const auto it = tasks_.find(task);
        if (it != tasks_.end() && lane_kind(it->second.spec.lane) == LaneKind::Serial) {
            inflight_serial_lanes_.erase(it->second.spec.lane);
        }
        return true;
    }

    /**
     * @brief 设置当前调度 tick 编号，用于 lane 饥饿检测。
     */
    void set_current_tick(std::uint64_t tick) noexcept { current_tick_ = tick; }

    /**
     * @brief 返回当前调度 tick 编号。
     */
    [[nodiscard]] std::uint64_t current_tick() const noexcept { return current_tick_; }

    /**
     * @brief 返回指定 lane 距离上次被调度已经过的 tick 数。
     *
     * 如果该 lane 从未被调度，返回 `UINT64_MAX`。
     */
    [[nodiscard]] std::uint64_t lane_starvation_ticks(LaneId lane) const noexcept {
        const auto it = lane_last_dispatched_tick_.find(lane);
        if (it == lane_last_dispatched_tick_.end()) {
            return (std::numeric_limits<std::uint64_t>::max)();
        }
        return current_tick_ - it->second;
    }

   private:
    struct TaskState {
        TaskSpec spec;
        std::optional<std::uint64_t> deadline_ms;
    };

    struct PeriodicState {
        std::chrono::milliseconds period{1};
        std::chrono::milliseconds next_deadline{0};
        std::uint64_t missed_periods{};
    };

    struct PendingAdmission {
        std::uint64_t scheduled_time_ms{};
        std::optional<std::uint64_t> period_ms;
        std::uint64_t missed_periods{};
    };

    [[nodiscard]] std::vector<TaskId> ready_parallel_order() const {
        std::map<LaneId, std::vector<TaskId>> by_lane;
        for (const auto task : ready_) {
            if (!can_admit(task)) {
                continue;
            }
            const auto it = tasks_.find(task);
            if (it != tasks_.end()) {
                by_lane[it->second.spec.lane].push_back(task);
            }
        }
        for (auto &[lane, lane_tasks] : by_lane) {
            (void)lane;
            std::sort(lane_tasks.begin(), lane_tasks.end(), [this](TaskId left, TaskId right) {
                return compare_ready_tasks(left, right);
            });
        }

        std::vector<TaskId> tasks;
        std::set<LaneId> reserved_serial_lanes;
        for (auto &[lane, lane_tasks] : by_lane) {
            switch (lane_kind(lane)) {
                case LaneKind::Serial:
                    if (inflight_serial_lanes_.contains(lane) ||
                        reserved_serial_lanes.contains(lane) || lane_tasks.empty()) {
                        break;
                    }
                    reserved_serial_lanes.insert(lane);
                    tasks.push_back(lane_tasks.front());
                    break;
                case LaneKind::Parallel:
                    tasks.insert(tasks.end(), lane_tasks.begin(), lane_tasks.end());
                    break;
            }
        }
        return tasks;
    }

    void record_periodic_due(TaskId task, std::chrono::milliseconds scheduled_time,
                             std::chrono::milliseconds period, std::uint64_t missed_from_deadline) {
        const auto scheduled_time_ms = millis_to_u64(scheduled_time);
        const auto period_ms = millis_to_u64(period);
        std::uint64_t counted_pending = 0;
        const auto pending_it = pending_.find(task);
        const auto pending_same_period = pending_it != pending_.end() &&
                                         pending_it->second.period_ms.has_value() &&
                                         *pending_it->second.period_ms == period_ms;
        if (pending_same_period) {
            counted_pending = pending_it->second.missed_periods;
        }

        auto missed_periods =
            missed_from_deadline + static_cast<std::uint64_t>(inflight_tasks_.contains(task));
        if (pending_same_period && scheduled_time_ms >= pending_it->second.scheduled_time_ms) {
            const auto delta = scheduled_time_ms - pending_it->second.scheduled_time_ms;
            missed_periods = saturating_add_u64(pending_it->second.missed_periods,
                                                delta / std::max<std::uint64_t>(1, period_ms));
        }

        const auto periodic_it = periodic_.find(task);
        if (periodic_it != periodic_.end()) {
            const auto additional_missed =
                missed_periods > counted_pending ? missed_periods - counted_pending : 0U;
            periodic_it->second.missed_periods =
                saturating_add_u64(periodic_it->second.missed_periods, additional_missed);
        }
        if (admission_open_ && !suspended_.contains(task) && tasks_.contains(task)) {
            ready_.insert(task);
            pending_[task] = PendingAdmission{
                .scheduled_time_ms = scheduled_time_ms,
                .period_ms = period_ms,
                .missed_periods = missed_periods,
            };
        }
    }

    [[nodiscard]] std::optional<TaskAdmission> admit_task(TaskId task) {
        if (!can_admit(task)) {
            return std::nullopt;
        }
        auto admission = admission_for(task);
        if (!admission.has_value()) {
            return std::nullopt;
        }
        ready_.erase(task);
        pending_.erase(task);
        inflight_tasks_.insert(task);
        if (lane_kind(admission->lane) == LaneKind::Serial) {
            inflight_serial_lanes_.insert(admission->lane);
        }
        lane_last_dispatched_tick_[admission->lane] = current_tick_;
        return admission;
    }

    [[nodiscard]] std::optional<TaskAdmission> admission_for(TaskId task) const {
        const auto task_it = tasks_.find(task);
        if (task_it == tasks_.end()) {
            return std::nullopt;
        }
        PendingAdmission pending{.scheduled_time_ms = millis_to_u64(now_)};
        const auto pending_it = pending_.find(task);
        if (pending_it != pending_.end()) {
            pending = pending_it->second;
        }
        const auto observed_time_ms = millis_to_u64(now_);
        return TaskAdmission{
            .task = task,
            .lane = task_it->second.spec.lane,
            .scheduled_time_ms = pending.scheduled_time_ms,
            .observed_time_ms = observed_time_ms,
            .period_ms = pending.period_ms,
            .deadline_ms = task_it->second.deadline_ms,
            .missed_periods = pending.missed_periods,
            .lateness_ms = observed_time_ms > pending.scheduled_time_ms
                               ? observed_time_ms - pending.scheduled_time_ms
                               : 0U,
        };
    }

    [[nodiscard]] bool can_admit(TaskId task) const {
        if (inflight_tasks_.contains(task) || suspended_.contains(task)) {
            return false;
        }
        const auto it = tasks_.find(task);
        if (it == tasks_.end()) {
            return false;
        }
        switch (lane_kind(it->second.spec.lane)) {
            case LaneKind::Serial:
                return !inflight_serial_lanes_.contains(it->second.spec.lane);
            case LaneKind::Parallel:
                return true;
        }
        return false;
    }

    [[nodiscard]] LaneKind lane_kind(LaneId lane) const {
        const auto it = lanes_.find(lane);
        return it == lanes_.end() ? LaneKind::Serial : it->second;
    }

    [[nodiscard]] bool compare_ready_tasks(TaskId left, TaskId right) const {
        const auto left_it = tasks_.find(left);
        const auto right_it = tasks_.find(right);
        if (left_it == tasks_.end() || right_it == tasks_.end()) {
            return left < right;
        }
        if (left_it->second.spec.priority != right_it->second.spec.priority) {
            return left_it->second.spec.priority < right_it->second.spec.priority;
        }
        return left_it->second.spec.id < right_it->second.spec.id;
    }

    [[nodiscard]] static std::uint64_t saturating_add_u64(std::uint64_t left,
                                                          std::uint64_t right) noexcept {
        const auto max = (std::numeric_limits<std::uint64_t>::max)();
        return max - left < right ? max : left + right;
    }

    [[nodiscard]] static std::chrono::milliseconds saturated_add(
        std::chrono::milliseconds left, std::chrono::milliseconds right) noexcept {
        const auto max = std::chrono::milliseconds::max().count();
        if (right.count() > 0 && max - left.count() < right.count()) {
            return std::chrono::milliseconds::max();
        }
        return left + right;
    }

    [[nodiscard]] static std::chrono::milliseconds saturated_mul(std::chrono::milliseconds period,
                                                                 std::uint64_t count) noexcept {
        const auto max = std::chrono::milliseconds::max().count();
        const auto period_count = period.count();
        if (period_count <= 0 || count == 0U) {
            return std::chrono::milliseconds{0};
        }
        if (static_cast<std::uint64_t>(max / period_count) < count) {
            return std::chrono::milliseconds::max();
        }
        return std::chrono::milliseconds{period_count * static_cast<std::int64_t>(count)};
    }

    [[nodiscard]] static std::uint64_t millis_to_u64(std::chrono::milliseconds value) noexcept {
        return value.count() < 0 ? 0U : static_cast<std::uint64_t>(value.count());
    }

    std::size_t worker_threads_;
    std::chrono::milliseconds now_{0};
    std::uint64_t current_tick_{0};
    bool admission_open_{true};
    std::map<LaneId, LaneKind> lanes_;
    std::map<TaskId, TaskState> tasks_;
    std::set<TaskId> ready_;
    std::map<TaskId, PendingAdmission> pending_;
    std::map<TaskId, PeriodicState> periodic_;
    std::set<TaskId> inflight_tasks_;
    std::set<TaskId> suspended_;
    std::set<LaneId> inflight_serial_lanes_;
    std::map<LaneId, std::uint64_t> lane_last_dispatched_tick_;
};

class ManualExecutor {
   public:
    using Runnable = std::function<void()>;

    void post(Runnable task) { ready_.push(std::move(task)); }

    std::size_t run_ready() {
        std::size_t count = 0;
        while (!ready_.empty()) {
            auto task = std::move(ready_.front());
            ready_.pop();
            task();
            ++count;
        }
        return count;
    }

    [[nodiscard]] bool empty() const noexcept { return ready_.empty(); }

   private:
    std::queue<Runnable> ready_;
};

class ScheduleAwaitable {
   public:
    explicit ScheduleAwaitable(ManualExecutor &executor) : executor_(executor) {}

    [[nodiscard]] bool await_ready() const noexcept { return false; }

    void await_suspend(std::coroutine_handle<> handle) const {
        executor_.post([handle]() mutable { handle.resume(); });
    }

    void await_resume() const noexcept {}

   private:
    ManualExecutor &executor_;
};

inline ScheduleAwaitable schedule_on(ManualExecutor &executor) {
    return ScheduleAwaitable{executor};
}

class DetachedTask {
   public:
    struct promise_type {
        DetachedTask get_return_object() noexcept { return DetachedTask{}; }
        std::suspend_never initial_suspend() const noexcept { return {}; }
        std::suspend_never final_suspend() const noexcept { return {}; }
        void return_void() const noexcept {}
        void unhandled_exception() const noexcept { std::terminate(); }
    };
};

/**
 * @brief worker job 提交结果。
 */
struct WorkerSubmitResult {
    TaskId task;      ///< 被提交或拒绝的 task id。
    bool accepted{};  ///< true 表示 job 已进入 worker 队列。

    /**
     * @brief 返回 job 是否已成功进入 worker 队列。
     */
    [[nodiscard]] bool submitted() const noexcept { return accepted; }
};

class WorkerPool {
   public:
    using Job = std::function<Status()>;
    using Completion = std::function<void(Status)>;

    explicit WorkerPool(std::size_t worker_threads)
        : worker_threads_(std::max<std::size_t>(1, worker_threads)) {
        workers_.reserve(worker_threads_);
        for (std::size_t index = 0; index < worker_threads_; ++index) {
            workers_.emplace_back([this]() { worker_loop(); });
        }
    }

    WorkerPool(const WorkerPool &) = delete;
    WorkerPool &operator=(const WorkerPool &) = delete;

    WorkerPool(WorkerPool &&) = delete;
    WorkerPool &operator=(WorkerPool &&) = delete;

    ~WorkerPool() { (void)shutdown(); }

    [[nodiscard]] std::size_t worker_threads() const noexcept { return worker_threads_; }

    bool spawn(Job job) {
        std::lock_guard lock(mutex_);
        if (!admission_open_) {
            return false;
        }
        jobs_.push(QueuedJob{.job = std::move(job)});
        ready_.notify_one();
        return true;
    }

    bool spawn_with_completion(Job job, Completion completion) {
        std::lock_guard lock(mutex_);
        if (!admission_open_) {
            return false;
        }
        jobs_.push(QueuedJob{.job = std::move(job), .completion = std::move(completion)});
        ready_.notify_one();
        return true;
    }

    /**
     * @brief 非阻塞提交带 typed output 的 task job。
     *
     * 提交成功后立即返回；worker 完成时向 completion queue 写入一条
     * `TaskRunOutput`。exception 会转换为 `Status::Error`，且不保留输出。
     */
    template <typename Outputs, typename Fn>
    WorkerSubmitResult submit_collect(TaskId task, WorkerCompletionQueue<Outputs> completions,
                                      Fn &&fn) {
        using FnType = std::decay_t<Fn>;
        struct CompletionSlot {
            std::mutex mutex;
            std::optional<TaskRunOutput<Outputs>> result;
        };

        auto slot = std::make_shared<CompletionSlot>();
        auto fn_shared = std::make_shared<FnType>(std::forward<Fn>(fn));
        const auto submitted = spawn_with_completion(
            [task, slot, fn_shared]() {
                auto outcome = std::invoke(*fn_shared);
                const auto status = outcome.status;
                {
                    std::lock_guard lock(slot->mutex);
                    slot->result = TaskRunOutput<Outputs>::from_outcome(task, std::move(outcome));
                }
                return status;
            },
            [task, slot, completions](Status status) {
                std::optional<TaskRunOutput<Outputs>> result;
                {
                    std::lock_guard lock(slot->mutex);
                    if (slot->result.has_value()) {
                        result.emplace(std::move(*slot->result));
                        slot->result.reset();
                    }
                }
                if (result.has_value()) {
                    completions.push(std::move(*result));
                } else {
                    completions.push(TaskRunOutput<Outputs>::without_outputs(task, status));
                }
            });
        return WorkerSubmitResult{.task = task, .accepted = submitted};
    }

    void close_admission() noexcept {
        std::lock_guard lock(mutex_);
        admission_open_ = false;
        ready_.notify_all();
    }

    Status drain() {
        std::unique_lock lock(mutex_);
        drained_.wait(lock, [this]() { return jobs_.empty() && active_ == 0U; });
        return status_;
    }

    Status shutdown() {
        close_admission();
        (void)drain();

        for (auto &worker : workers_) {
            if (worker.joinable()) {
                worker.join();
            }
        }

        return drain();
    }

   private:
    struct QueuedJob {
        Job job;
        Completion completion;
    };

    void worker_loop() {
        while (true) {
            QueuedJob queued_job;
            {
                std::unique_lock lock(mutex_);
                ready_.wait(lock, [this]() { return !jobs_.empty() || !admission_open_; });
                if (jobs_.empty() && !admission_open_) {
                    return;
                }
                queued_job = std::move(jobs_.front());
                jobs_.pop();
                ++active_;
            }

            Status status = Status::Error;
            try {
                status = queued_job.job();
            } catch (...) {
                status = Status::Error;
            }
            if (queued_job.completion) {
                try {
                    queued_job.completion(status);
                } catch (...) {
                }
            }

            std::lock_guard lock(mutex_);
            if (status == Status::Error) {
                status_ = Status::Error;
            }
            if (active_ > 0U) {
                --active_;
            }
            if (jobs_.empty() && active_ == 0U) {
                drained_.notify_all();
            }
        }
    }

    std::size_t worker_threads_;
    mutable std::mutex mutex_;
    std::condition_variable ready_;
    std::condition_variable drained_;
    bool admission_open_{true};
    std::queue<QueuedJob> jobs_;
    std::size_t active_{};
    Status status_{Status::Ok};
    std::vector<std::thread> workers_;
};

}  // namespace flowrt
