#pragma once

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <coroutine>
#include <cstdint>
#include <exception>
#include <flowrt/core.hpp>
#include <functional>
#include <future>
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

enum class ScheduleEvent {
    Data,
    Timer,
    Shutdown,
};

class ScheduleWaiter {
   public:
    ScheduleWaiter() : state_(std::make_shared<State>()) {}

    void notify_data() const {
        notify_data_with_time(std::nullopt);
    }

    void notify_data_at_ms(std::uint64_t time_ms) const {
        notify_data_with_time(time_ms);
    }

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

    explicit ReadyBatch(std::vector<TaskId> tasks) : tasks_(std::move(tasks)) {}

    [[nodiscard]] bool empty() const noexcept { return tasks_.empty(); }

    [[nodiscard]] std::size_t size() const noexcept { return tasks_.size(); }

    [[nodiscard]] const std::vector<TaskId> &tasks() const noexcept { return tasks_; }

    template <typename Fn>
    std::vector<TaskRunResult> run(WorkerPool &worker_pool, Fn &&fn) &&;

    /**
     * @brief 在当前 scheduler 线程内执行 ready batch。
     */
    template <typename Fn>
    std::vector<TaskRunResult> run_local(Fn &&fn) &&;

    /**
     * @brief 通过 worker pool 执行 ready batch，并收集可提交输出。
     *
     * 输出集合会从 worker 线程交还 scheduler 线程；需要跨线程移动的约束由 generated
     * shell 通过具体 `Outputs` 类型承担。非 `Ok` 和 exception 路径会丢弃输出。
     */
    template <typename Fn>
    auto run_collect(WorkerPool &worker_pool, Fn &&fn)
        && -> std::vector<TaskRunOutput<
               typename std::invoke_result_t<std::decay_t<Fn> &, TaskId>::outputs_type>>;

    /**
     * @brief 在当前 scheduler 线程内执行 ready batch，并收集可提交输出。
     *
     * 该路径不复制 callable，也不要求输出集合满足跨线程移动约束。非 `Ok` 和 exception
     * 路径会丢弃输出。
     */
    template <typename Fn>
    auto run_local_collect(Fn &&fn)
        && -> std::vector<TaskRunOutput<typename std::invoke_result_t<Fn &, TaskId>::outputs_type>>;

   private:
    std::vector<TaskId> tasks_;
};

class DeterministicExecutor {
   public:
    explicit DeterministicExecutor(std::size_t worker_threads)
        : worker_threads_(std::max<std::size_t>(1, worker_threads)) {}

    [[nodiscard]] std::size_t worker_threads() const noexcept { return worker_threads_; }

    void add_lane(LaneId lane, LaneKind kind) { lanes_[lane] = kind; }

    void add_task(TaskSpec spec) { tasks_[spec.id] = spec; }

    void add_periodic(PeriodicSpec spec) {
        const auto period = std::max(spec.period, std::chrono::milliseconds{1});
        periodic_[spec.task] = PeriodicState{
            .period = period,
            .next_deadline = now_ + period,
            .missed_periods = 0,
        };
    }

    void wake(TaskId task) {
        if (admission_open_ && tasks_.contains(task)) {
            ready_.insert(task);
        }
    }

    void close_admission() noexcept { admission_open_ = false; }

    [[nodiscard]] bool is_drained() const noexcept { return ready_.empty(); }

    void advance_to(std::chrono::milliseconds now) {
        now_ = now;
        std::vector<TaskId> due;
        for (auto &[task, state] : periodic_) {
            if (now_ < state.next_deadline) {
                continue;
            }
            const auto elapsed = now_ - state.next_deadline;
            const auto missed = static_cast<std::uint64_t>(elapsed / state.period);
            state.missed_periods += missed;
            state.next_deadline += state.period * static_cast<std::int64_t>(missed + 1U);
            due.push_back(task);
        }
        for (const auto task : due) {
            wake(task);
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

    template <typename Fn>
    auto run_ready(Fn &&fn) {
        using Result = std::invoke_result_t<Fn, TaskId>;
        std::vector<Result> results;
        const auto ordered = ready_order();
        for (const auto task : ordered) {
            ready_.erase(task);
            if (const auto it = tasks_.find(task); it != tasks_.end()) {
                lane_last_dispatched_tick_[it->second.lane] = current_tick_;
            }
            results.push_back(std::invoke(fn, task));
        }
        return results;
    }

    [[nodiscard]] ReadyBatch take_ready_batch() {
        auto tasks = ready_parallel_order();
        for (const auto task : tasks) {
            ready_.erase(task);
            if (const auto it = tasks_.find(task); it != tasks_.end()) {
                lane_last_dispatched_tick_[it->second.lane] = current_tick_;
            }
        }
        return ReadyBatch{std::move(tasks)};
    }

    template <typename Fn>
    std::vector<TaskRunResult> run_ready_parallel(WorkerPool &worker_pool, Fn &&fn) {
        return take_ready_batch().run(worker_pool, std::forward<Fn>(fn));
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
    struct PeriodicState {
        std::chrono::milliseconds period{1};
        std::chrono::milliseconds next_deadline{0};
        std::uint64_t missed_periods{};
    };

    [[nodiscard]] std::vector<TaskId> ready_order() const {
        std::map<LaneId, std::vector<TaskId>> by_lane;
        for (const auto task : ready_) {
            by_lane[tasks_.at(task).lane].push_back(task);
        }
        for (auto &[lane, lane_tasks] : by_lane) {
            (void)lane;
            std::sort(lane_tasks.begin(), lane_tasks.end(), [this](TaskId left, TaskId right) {
                const auto &left_spec = tasks_.at(left);
                const auto &right_spec = tasks_.at(right);
                if (left_spec.priority != right_spec.priority) {
                    return left_spec.priority < right_spec.priority;
                }
                return left_spec.id < right_spec.id;
            });
        }

        std::vector<TaskId> tasks;
        tasks.reserve(ready_.size());
        bool has_ready = true;
        while (has_ready) {
            has_ready = false;
            for (auto &[lane, lane_tasks] : by_lane) {
                (void)lane;
                if (!lane_tasks.empty()) {
                    tasks.push_back(lane_tasks.front());
                    lane_tasks.erase(lane_tasks.begin());
                    has_ready = true;
                }
            }
        }
        return tasks;
    }

    [[nodiscard]] std::vector<TaskId> ready_parallel_order() const {
        std::map<LaneId, std::vector<TaskId>> by_lane;
        for (const auto task : ready_) {
            by_lane[tasks_.at(task).lane].push_back(task);
        }
        for (auto &[lane, lane_tasks] : by_lane) {
            (void)lane;
            std::sort(lane_tasks.begin(), lane_tasks.end(), [this](TaskId left, TaskId right) {
                const auto &left_spec = tasks_.at(left);
                const auto &right_spec = tasks_.at(right);
                if (left_spec.priority != right_spec.priority) {
                    return left_spec.priority < right_spec.priority;
                }
                return left_spec.id < right_spec.id;
            });
        }

        std::vector<TaskId> tasks;
        tasks.reserve(by_lane.size());
        for (auto &[lane, lane_tasks] : by_lane) {
            (void)lane;
            if (!lane_tasks.empty()) {
                tasks.push_back(lane_tasks.front());
            }
        }
        return tasks;
    }

    std::size_t worker_threads_;
    std::chrono::milliseconds now_{0};
    std::uint64_t current_tick_{0};
    bool admission_open_{true};
    std::map<LaneId, LaneKind> lanes_;
    std::map<TaskId, TaskSpec> tasks_;
    std::set<TaskId> ready_;
    std::map<TaskId, PeriodicState> periodic_;
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

template <typename Fn>
std::vector<TaskRunResult> ReadyBatch::run(WorkerPool &worker_pool, Fn &&fn) && {
    if (tasks_.empty()) {
        return {};
    }

    struct BatchState {
        std::mutex mutex;
        std::condition_variable ready;
        std::size_t remaining{};
        std::vector<std::optional<TaskRunResult>> results;
    };

    auto state = std::make_shared<BatchState>();
    state->remaining = tasks_.size();
    state->results.resize(tasks_.size());
    auto fn_shared = std::make_shared<std::decay_t<Fn>>(std::forward<Fn>(fn));

    for (std::size_t index = 0; index < tasks_.size(); ++index) {
        const auto task = tasks_[index];
        auto submitted = worker_pool.spawn_with_completion(
            [fn_shared, task]() { return std::invoke(*fn_shared, task); },
            [state, index, task](Status status) {
                std::lock_guard lock(state->mutex);
                state->results[index] = TaskRunResult{.task = task, .status = status};
                if (state->remaining > 0U) {
                    --state->remaining;
                }
                if (state->remaining == 0U) {
                    state->ready.notify_all();
                }
            });
        if (submitted) {
            continue;
        }

        Status status = Status::Error;
        try {
            status = std::invoke(*fn_shared, task);
        } catch (...) {
            status = Status::Error;
        }
        std::lock_guard lock(state->mutex);
        state->results[index] = TaskRunResult{.task = task, .status = status};
        if (state->remaining > 0U) {
            --state->remaining;
        }
        if (state->remaining == 0U) {
            state->ready.notify_all();
        }
    }

    std::unique_lock lock(state->mutex);
    state->ready.wait(lock, [&state]() { return state->remaining == 0U; });

    std::vector<TaskRunResult> results;
    results.reserve(state->results.size());
    for (const auto &result : state->results) {
        results.push_back(*result);
    }
    return results;
}

template <typename Fn>
std::vector<TaskRunResult> ReadyBatch::run_local(Fn &&fn) && {
    std::vector<TaskRunResult> results;
    results.reserve(tasks_.size());
    for (const auto task : tasks_) {
        Status status = Status::Error;
        try {
            status = std::invoke(fn, task);
        } catch (...) {
            status = Status::Error;
        }
        results.push_back(TaskRunResult{.task = task, .status = status});
    }
    return results;
}

template <typename Fn>
auto ReadyBatch::run_collect(WorkerPool &worker_pool, Fn &&fn)
    && -> std::vector<
           TaskRunOutput<typename std::invoke_result_t<std::decay_t<Fn> &, TaskId>::outputs_type>> {
    using FnType = std::decay_t<Fn>;
    using Outcome = std::invoke_result_t<FnType &, TaskId>;
    using Outputs = typename Outcome::outputs_type;

    if (tasks_.empty()) {
        return {};
    }

    struct BatchState {
        std::mutex mutex;
        std::condition_variable ready;
        std::size_t remaining{};
        std::vector<std::optional<TaskRunOutput<Outputs>>> results;
    };

    auto state = std::make_shared<BatchState>();
    state->remaining = tasks_.size();
    state->results.resize(tasks_.size());
    auto fn_shared = std::make_shared<FnType>(std::forward<Fn>(fn));
    auto record_result = [state](std::size_t index, TaskRunOutput<Outputs> result) {
        std::lock_guard lock(state->mutex);
        if (state->results[index].has_value()) {
            return;
        }
        state->results[index] = std::move(result);
        if (state->remaining > 0U) {
            --state->remaining;
        }
        if (state->remaining == 0U) {
            state->ready.notify_all();
        }
    };

    for (std::size_t index = 0; index < tasks_.size(); ++index) {
        const auto task = tasks_[index];
        auto submitted = worker_pool.spawn_with_completion(
            [fn_shared, record_result, index, task]() {
                auto outcome = std::invoke(*fn_shared, task);
                const auto status = outcome.status;
                record_result(index,
                              TaskRunOutput<Outputs>::from_outcome(task, std::move(outcome)));
                return status;
            },
            [record_result, index, task](Status status) {
                record_result(index, TaskRunOutput<Outputs>::without_outputs(task, status));
            });
        if (submitted) {
            continue;
        }

        try {
            auto outcome = std::invoke(*fn_shared, task);
            record_result(index, TaskRunOutput<Outputs>::from_outcome(task, std::move(outcome)));
        } catch (...) {
            record_result(index, TaskRunOutput<Outputs>::without_outputs(task, Status::Error));
        }
    }

    std::unique_lock lock(state->mutex);
    state->ready.wait(lock, [&state]() { return state->remaining == 0U; });

    std::vector<TaskRunOutput<Outputs>> results;
    results.reserve(state->results.size());
    for (auto &result : state->results) {
        results.push_back(std::move(*result));
    }
    return results;
}

template <typename Fn>
auto ReadyBatch::run_local_collect(Fn &&fn)
    && -> std::vector<TaskRunOutput<typename std::invoke_result_t<Fn &, TaskId>::outputs_type>> {
    using Outcome = std::invoke_result_t<Fn &, TaskId>;
    using Outputs = typename Outcome::outputs_type;

    std::vector<TaskRunOutput<Outputs>> results;
    results.reserve(tasks_.size());
    for (const auto task : tasks_) {
        try {
            auto outcome = std::invoke(fn, task);
            results.push_back(TaskRunOutput<Outputs>::from_outcome(task, std::move(outcome)));
        } catch (...) {
            results.push_back(TaskRunOutput<Outputs>::without_outputs(task, Status::Error));
        }
    }
    return results;
}

}  // namespace flowrt
