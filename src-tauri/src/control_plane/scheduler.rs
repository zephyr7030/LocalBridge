use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::domain::RpcRequestId;
use crate::domain::{McpSessionId, RequestKey, TaskId};

pub(crate) const MAX_WORK_QUEUE: usize = 32;
pub(crate) const MAX_OBSERVATION_ACTIVE: usize = 16;
pub(crate) const MAX_CONTROL_ACTIVE: usize = 16;
const FOREGROUND_WORK_SLOTS: usize = 1;
static TICKET_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchedulerLane {
    Observation,
    Control,
    Work,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchedulerAdmissionError {
    QueueCapacityExceeded,
    ImmediateCapacityExceeded,
    Cancelled,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerSnapshot {
    pub observation_active: usize,
    pub observation_capacity: usize,
    pub control_active: usize,
    pub control_capacity: usize,
    #[serde(rename = "foreground_work_running")]
    pub work_running: usize,
    #[serde(rename = "queue_depth")]
    pub work_queued: usize,
    #[serde(rename = "queue_capacity")]
    pub work_capacity: usize,
    pub rejected_total: u64,
}

impl SchedulerSnapshot {
    pub(crate) const fn idle() -> Self {
        Self {
            observation_active: 0,
            observation_capacity: MAX_OBSERVATION_ACTIVE,
            control_active: 0,
            control_capacity: MAX_CONTROL_ACTIVE,
            work_running: 0,
            work_queued: 0,
            work_capacity: MAX_WORK_QUEUE,
            rejected_total: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct QueuedWork {
    ticket: u64,
    owner_session: McpSessionId,
    task_id: TaskId,
    request: RequestKey,
}

#[derive(Debug, Default)]
struct SchedulerState {
    observation_active: usize,
    control_active: usize,
    work_running: usize,
    queue: VecDeque<QueuedWork>,
    cancelled: HashSet<u64>,
    rejected_total: u64,
    closed: bool,
}

#[derive(Debug, Default)]
struct SchedulerInner {
    state: Mutex<SchedulerState>,
    changed: Condvar,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Scheduler(Arc<SchedulerInner>);

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum SchedulerPermit {
    Immediate(ImmediatePermit),
    Work(WorkPermit),
}

#[derive(Debug)]
pub(crate) struct ImmediatePermit {
    scheduler: Scheduler,
    lane: SchedulerLane,
}

#[derive(Debug)]
pub(crate) struct WorkPermit {
    scheduler: Scheduler,
    released: bool,
}

impl Scheduler {
    pub(crate) fn enter_immediate(
        &self,
        lane: SchedulerLane,
    ) -> Result<SchedulerPermit, SchedulerAdmissionError> {
        assert!(lane != SchedulerLane::Work);
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Err(SchedulerAdmissionError::Closed);
        }
        match lane {
            SchedulerLane::Observation => {
                if state.observation_active >= MAX_OBSERVATION_ACTIVE {
                    state.rejected_total = state.rejected_total.saturating_add(1);
                    return Err(SchedulerAdmissionError::ImmediateCapacityExceeded);
                }
                state.observation_active = state.observation_active.saturating_add(1)
            }
            SchedulerLane::Control => {
                if state.control_active >= MAX_CONTROL_ACTIVE {
                    state.rejected_total = state.rejected_total.saturating_add(1);
                    return Err(SchedulerAdmissionError::ImmediateCapacityExceeded);
                }
                state.control_active = state.control_active.saturating_add(1)
            }
            SchedulerLane::Work => unreachable!(),
        }
        drop(state);
        Ok(SchedulerPermit::Immediate(ImmediatePermit {
            scheduler: self.clone(),
            lane,
        }))
    }

    pub(crate) fn admit_work_for_request(
        &self,
        owner_session: McpSessionId,
        task_id: TaskId,
        request: RequestKey,
    ) -> Result<SchedulerPermit, SchedulerAdmissionError> {
        let ticket = TICKET_GENERATION.fetch_add(1, Ordering::Relaxed);
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Err(SchedulerAdmissionError::Closed);
        }
        if state.work_running < FOREGROUND_WORK_SLOTS && state.queue.is_empty() {
            state.work_running += 1;
            return Ok(SchedulerPermit::Work(WorkPermit {
                scheduler: self.clone(),
                released: false,
            }));
        }
        if state.queue.len() >= MAX_WORK_QUEUE {
            state.rejected_total = state.rejected_total.saturating_add(1);
            return Err(SchedulerAdmissionError::QueueCapacityExceeded);
        }
        state.queue.push_back(QueuedWork {
            ticket,
            owner_session,
            task_id,
            request,
        });

        loop {
            state = self
                .0
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.cancelled.remove(&ticket) {
                return Err(SchedulerAdmissionError::Cancelled);
            }
            let is_front = state
                .queue
                .front()
                .is_some_and(|queued| queued.ticket == ticket);
            if is_front && state.work_running < FOREGROUND_WORK_SLOTS {
                state.queue.pop_front();
                state.work_running += 1;
                return Ok(SchedulerPermit::Work(WorkPermit {
                    scheduler: self.clone(),
                    released: false,
                }));
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn admit_work(
        &self,
        owner_session: McpSessionId,
        task_id: TaskId,
    ) -> Result<SchedulerPermit, SchedulerAdmissionError> {
        let request = RequestKey::new(
            owner_session.clone(),
            RpcRequestId::String(format!("test-{}", task_id.as_str())),
        );
        self.admit_work_for_request(owner_session, task_id, request)
    }

    pub(crate) fn cancel_queued_request(&self, request: &RequestKey) -> Option<TaskId> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = state
            .queue
            .iter()
            .position(|queued| &queued.request == request)?;
        let queued = state
            .queue
            .remove(index)
            .expect("located queued work remains present");
        state.cancelled.insert(queued.ticket);
        self.0.changed.notify_all();
        Some(queued.task_id)
    }

    pub(crate) fn close(&self) -> Vec<TaskId> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.closed = true;
        let mut cancelled = Vec::with_capacity(state.queue.len());
        while let Some(queued) = state.queue.pop_front() {
            state.cancelled.insert(queued.ticket);
            cancelled.push(queued.task_id);
        }
        self.0.changed.notify_all();
        cancelled
    }

    pub(crate) fn cancel_queued_by_session(&self, owner: &McpSessionId) -> Vec<TaskId> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut cancelled_tasks = Vec::new();
        let mut retained = VecDeque::with_capacity(state.queue.len());
        while let Some(queued) = state.queue.pop_front() {
            if &queued.owner_session == owner {
                state.cancelled.insert(queued.ticket);
                cancelled_tasks.push(queued.task_id);
            } else {
                retained.push_back(queued);
            }
        }
        state.queue = retained;
        self.0.changed.notify_all();
        cancelled_tasks
    }

    pub(crate) fn cancel_queued_task(&self, owner: &McpSessionId, task_id: &TaskId) -> bool {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(index) = state
            .queue
            .iter()
            .position(|queued| &queued.owner_session == owner && &queued.task_id == task_id)
        else {
            return false;
        };
        let queued = state
            .queue
            .remove(index)
            .expect("located queued work remains present");
        state.cancelled.insert(queued.ticket);
        self.0.changed.notify_all();
        true
    }

    pub(crate) fn snapshot(&self) -> SchedulerSnapshot {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        SchedulerSnapshot {
            observation_active: state.observation_active,
            observation_capacity: MAX_OBSERVATION_ACTIVE,
            control_active: state.control_active,
            control_capacity: MAX_CONTROL_ACTIVE,
            work_running: state.work_running,
            work_queued: state.queue.len(),
            work_capacity: MAX_WORK_QUEUE,
            rejected_total: state.rejected_total,
        }
    }
}

impl Drop for ImmediatePermit {
    fn drop(&mut self) {
        let mut state = self
            .scheduler
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.lane {
            SchedulerLane::Observation => {
                state.observation_active = state.observation_active.saturating_sub(1)
            }
            SchedulerLane::Control => state.control_active = state.control_active.saturating_sub(1),
            SchedulerLane::Work => unreachable!(),
        }
    }
}

impl Drop for WorkPermit {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let mut state = self
            .scheduler
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.work_running = state.work_running.saturating_sub(1);
        self.released = true;
        self.scheduler.0.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn multi_window_work_queue_is_bounded_fifo_with_one_foreground_slot() {
        let scheduler = Scheduler::default();
        let first = scheduler
            .admit_work(McpSessionId::new("a"), TaskId::new("a"))
            .unwrap();
        let (tx, rx) = mpsc::channel();
        let b = scheduler.clone();
        let tx_b = tx.clone();
        let worker_b = thread::spawn(move || {
            let _permit = b
                .admit_work(McpSessionId::new("b"), TaskId::new("b"))
                .unwrap();
            tx_b.send("b").unwrap();
            thread::sleep(Duration::from_millis(25));
        });
        while scheduler.snapshot().work_queued != 1 {
            thread::yield_now();
        }
        let c = scheduler.clone();
        let worker_c = thread::spawn(move || {
            let _permit = c
                .admit_work(McpSessionId::new("c"), TaskId::new("c"))
                .unwrap();
            tx.send("c").unwrap();
        });
        while scheduler.snapshot().work_queued != 2 {
            thread::yield_now();
        }
        drop(first);
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "b");
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "c");
        worker_b.join().unwrap();
        worker_c.join().unwrap();
    }

    #[test]
    fn control_lane_is_not_blocked_by_running_work() {
        let scheduler = Scheduler::default();
        let _work = scheduler
            .admit_work(McpSessionId::new("a"), TaskId::new("a"))
            .unwrap();
        let control = scheduler.enter_immediate(SchedulerLane::Control).unwrap();
        assert_eq!(scheduler.snapshot().control_active, 1);
        drop(control);
        assert_eq!(scheduler.snapshot().control_active, 0);
    }

    #[test]
    fn cancelling_a_session_removes_only_its_queued_work() {
        let scheduler = Scheduler::default();
        let first = scheduler
            .admit_work(McpSessionId::new("running"), TaskId::new("running"))
            .unwrap();
        let queued_scheduler = scheduler.clone();
        let queued = thread::spawn(move || {
            queued_scheduler.admit_work(McpSessionId::new("a"), TaskId::new("a"))
        });
        while scheduler.snapshot().work_queued != 1 {
            thread::yield_now();
        }
        assert_eq!(
            scheduler.cancel_queued_by_session(&McpSessionId::new("b")),
            Vec::<TaskId>::new()
        );
        assert_eq!(
            scheduler.cancel_queued_by_session(&McpSessionId::new("a")),
            vec![TaskId::new("a")]
        );
        assert!(matches!(
            queued.join().unwrap(),
            Err(SchedulerAdmissionError::Cancelled)
        ));
        drop(first);
    }

    #[test]
    fn cancelling_one_task_never_cancels_a_sibling_task() {
        let scheduler = Scheduler::default();
        let first = scheduler
            .admit_work(McpSessionId::new("running"), TaskId::new("running"))
            .unwrap();
        let queued_scheduler = scheduler.clone();
        let queued_a = thread::spawn(move || {
            queued_scheduler.admit_work(McpSessionId::new("owner"), TaskId::new("a"))
        });
        while scheduler.snapshot().work_queued != 1 {
            thread::yield_now();
        }
        let queued_scheduler = scheduler.clone();
        let queued_b = thread::spawn(move || {
            queued_scheduler.admit_work(McpSessionId::new("owner"), TaskId::new("b"))
        });
        while scheduler.snapshot().work_queued != 2 {
            thread::yield_now();
        }

        assert!(scheduler.cancel_queued_task(&McpSessionId::new("owner"), &TaskId::new("a")));
        assert_eq!(scheduler.snapshot().work_queued, 1);
        assert!(matches!(
            queued_a.join().unwrap(),
            Err(SchedulerAdmissionError::Cancelled)
        ));

        drop(first);
        assert!(queued_b.join().unwrap().is_ok());
    }

    #[test]
    fn queued_work_is_cancelled_by_session_scoped_request_identity() {
        let scheduler = Scheduler::default();
        let running = scheduler
            .admit_work(McpSessionId::new("running"), TaskId::new("running"))
            .unwrap();
        let request = RequestKey::new(McpSessionId::new("owner"), RpcRequestId::Number(7));
        let queued_scheduler = scheduler.clone();
        let queued_request = request.clone();
        let queued = thread::spawn(move || {
            queued_scheduler.admit_work_for_request(
                McpSessionId::new("owner"),
                TaskId::new("queued"),
                queued_request,
            )
        });
        while scheduler.snapshot().work_queued != 1 {
            thread::yield_now();
        }

        assert_eq!(
            scheduler.cancel_queued_request(&request),
            Some(TaskId::new("queued"))
        );
        assert!(matches!(
            queued.join().unwrap(),
            Err(SchedulerAdmissionError::Cancelled)
        ));
        drop(running);
    }

    #[test]
    fn closing_scheduler_cancels_queued_work_and_rejects_late_admission() {
        let scheduler = Scheduler::default();
        let running = scheduler
            .admit_work(McpSessionId::new("running"), TaskId::new("running"))
            .unwrap();
        let queued_scheduler = scheduler.clone();
        let queued = thread::spawn(move || {
            queued_scheduler.admit_work(McpSessionId::new("owner"), TaskId::new("queued"))
        });
        while scheduler.snapshot().work_queued != 1 {
            thread::yield_now();
        }

        assert_eq!(scheduler.close(), vec![TaskId::new("queued")]);
        assert!(matches!(
            queued.join().unwrap(),
            Err(SchedulerAdmissionError::Cancelled)
        ));
        assert!(matches!(
            scheduler.admit_work(McpSessionId::new("late"), TaskId::new("late")),
            Err(SchedulerAdmissionError::Closed)
        ));
        assert!(matches!(
            scheduler.enter_immediate(SchedulerLane::Observation),
            Err(SchedulerAdmissionError::Closed)
        ));
        drop(running);
    }

    #[test]
    fn full_work_queue_returns_queue_capacity_exceeded_and_is_observable() {
        let scheduler = Scheduler::default();
        let _running = scheduler
            .admit_work(McpSessionId::new("running"), TaskId::new("running"))
            .unwrap();
        {
            let mut state = scheduler
                .0
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for index in 0..MAX_WORK_QUEUE {
                state.queue.push_back(QueuedWork {
                    ticket: index as u64 + 10_000,
                    owner_session: McpSessionId::new(format!("session-{index}")),
                    task_id: TaskId::new(format!("task-{index}")),
                    request: RequestKey::new(
                        McpSessionId::new(format!("session-{index}")),
                        RpcRequestId::Number(index as i64),
                    ),
                });
            }
        }
        assert!(matches!(
            scheduler.admit_work(McpSessionId::new("overflow"), TaskId::new("overflow")),
            Err(SchedulerAdmissionError::QueueCapacityExceeded)
        ));
        let snapshot = scheduler.snapshot();
        assert_eq!(snapshot.work_queued, MAX_WORK_QUEUE);
        assert_eq!(snapshot.rejected_total, 1);
    }
}
