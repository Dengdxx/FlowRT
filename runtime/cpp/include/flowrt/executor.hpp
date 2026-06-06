#pragma once

#include <algorithm>
#include <chrono>
#include <condition_variable>
#include <coroutine>
#include <cstdint>
#include <exception>
#include <flowrt/core.hpp>
#include <functional>
#include <map>
#include <mutex>
#include <optional>
#include <queue>
#include <set>
#include <memory>
#include <thread>
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

enum class ScheduleEvent {
    Data,
    Timer,
    Shutdown,
};

class ScheduleWaiter {
   public:
    ScheduleWaiter() : state_(std::make_shared<State>()) {}

    void notify_data() const {
        std::lock_guard lock(state_->mutex);
        ++state_->data_generation;
        state_->ready.notify_all();
    }

    [[nodiscard]] std::uint64_t data_generation() const {
        std::lock_guard lock(state_->mutex);
        return state_->data_generation;
    }

    [[nodiscard]] ScheduleEvent wait_until(std::optional<std::chrono::steady_clock::time_point> deadline,
                                           const ShutdownToken &shutdown) {
        return wait_until_after(data_generation(), deadline, shutdown);
    }

    [[nodiscard]] ScheduleEvent wait_until_after(std::uint64_t seen_generation,
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
    };

    std::shared_ptr<State> state_;
};

class DeterministicExecutor {
   public:
    explicit DeterministicExecutor(std::size_t worker_threads) : worker_threads_(std::max<std::size_t>(1, worker_threads)) {}

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
            results.push_back(std::invoke(fn, task));
        }
        return results;
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

    std::size_t worker_threads_;
    std::chrono::milliseconds now_{0};
    bool admission_open_{true};
    std::map<LaneId, LaneKind> lanes_;
    std::map<TaskId, TaskSpec> tasks_;
    std::set<TaskId> ready_;
    std::map<TaskId, PeriodicState> periodic_;
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

    void await_suspend(std::coroutine_handle<> handle) const { executor_.post([handle]() mutable { handle.resume(); }); }

    void await_resume() const noexcept {}

   private:
    ManualExecutor &executor_;
};

inline ScheduleAwaitable schedule_on(ManualExecutor &executor) { return ScheduleAwaitable{executor}; }

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

    explicit WorkerPool(std::size_t worker_threads) : worker_threads_(std::max<std::size_t>(1, worker_threads)) {
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
        jobs_.push(std::move(job));
        ready_.notify_one();
        return true;
    }

    Status shutdown() {
        {
            std::unique_lock lock(mutex_);
            admission_open_ = false;
            ready_.notify_all();
            drained_.wait(lock, [this]() { return jobs_.empty() && active_ == 0U; });
        }

        for (auto &worker : workers_) {
            if (worker.joinable()) {
                worker.join();
            }
        }

        std::lock_guard lock(mutex_);
        return status_;
    }

   private:
    void worker_loop() {
        while (true) {
            Job job;
            {
                std::unique_lock lock(mutex_);
                ready_.wait(lock, [this]() { return !jobs_.empty() || !admission_open_; });
                if (jobs_.empty() && !admission_open_) {
                    return;
                }
                job = std::move(jobs_.front());
                jobs_.pop();
                ++active_;
            }

            const auto status = job();

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
    std::queue<Job> jobs_;
    std::size_t active_{};
    Status status_{Status::Ok};
    std::vector<std::thread> workers_;
};

}  // namespace flowrt
