//! Roblox/Luau inspired Coroutine Task Scheduler for Microkernel

use crate::event::Event;

/// Result of a task resumption, mapping directly to Luau coroutine/task behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// `coroutine.yield()`: Pauses the task until the next executor cycle or event.
    Yielded,
    /// `task.wait(n)`: Pauses the task for `n` ticks.
    Wait(u32),
    /// `task.defer()`: Resumes the task at the end of the current or next cycle.
    Deferred,
    /// Task has completed execution and is dead.
    Dead,
}

/// A microkernel task acting as a state-machine coroutine.
pub trait Task<C> {
    fn name(&self) -> &'static str;

    /// `coroutine.resume()`: called by the executor to advance the task state.
    fn resume(&mut self, ctx: &mut C, event: Option<&Event>) -> TaskState;
}

pub struct TaskControlBlock<'a, C> {
    pub task_ptr: Option<*mut (dyn Task<C> + 'a)>,
    pub state: TaskState,
    pub wait_counter: u32,
}

/// Static Micro-Executor that runs registered tasks in a zero-alloc loop.
pub struct TaskScheduler<'a, C, const N: usize> {
    pub tcbs: [TaskControlBlock<'a, C>; N],
    task_count: usize,
}

const fn empty_tcb<'a, C>() -> TaskControlBlock<'a, C> {
    TaskControlBlock {
        task_ptr: None,
        state: TaskState::Dead,
        wait_counter: 0,
    }
}

impl<'a, C, const N: usize> TaskScheduler<'a, C, N> {
    pub const fn new() -> Self {
        let mut tcbs: [core::mem::MaybeUninit<TaskControlBlock<'a, C>>; N] =
            unsafe { core::mem::MaybeUninit::uninit().assume_init() };

        let mut i = 0;
        while i < N {
            tcbs[i] = core::mem::MaybeUninit::new(empty_tcb());
            i += 1;
        }

        let final_tcbs: [TaskControlBlock<'a, C>; N] = unsafe { core::mem::transmute_copy(&tcbs) };
        Self {
            tcbs: final_tcbs,
            task_count: 0,
        }
    }

    /// `task.spawn()`: Registers and immediately prepares a task for execution.
    pub fn spawn(&mut self, task: &'a mut dyn Task<C>) -> Result<(), ()> {
        if self.task_count >= N {
            return Err(());
        }
        self.tcbs[self.task_count] = TaskControlBlock {
            task_ptr: Some(task as *mut (dyn Task<C> + 'a)),
            state: TaskState::Yielded,
            wait_counter: 0,
        };
        self.task_count += 1;
        Ok(())
    }

    /// Dispatch a system event and tick to all registered tasks.
    pub fn tick(&mut self, ctx: &mut C, event: Option<&Event>) {
        let is_tick = matches!(event, Some(Event::Tick));

        for i in 0..self.task_count {
            let tcb = &mut self.tcbs[i];

            if tcb.state == TaskState::Dead {
                continue;
            }

            // Handle `task.wait(n)` decrement
            if let TaskState::Wait(ref mut ticks) = tcb.state {
                if is_tick {
                    if *ticks > 0 {
                        *ticks -= 1;
                    }
                    if *ticks == 0 {
                        tcb.state = TaskState::Yielded;
                    }
                }
                if tcb.state != TaskState::Yielded {
                    continue;
                }
            }

            // Handle `task.defer()`
            if tcb.state == TaskState::Deferred {
                tcb.state = TaskState::Yielded;
            }

            // Resume task
            if tcb.state == TaskState::Yielded {
                if let Some(task_ptr) = tcb.task_ptr {
                    unsafe {
                        let task = &mut *task_ptr;
                        tcb.state = task.resume(ctx, event);
                    }
                }
            }
        }
    }

    pub fn active_task_count(&self) -> u8 {
        let mut count = 0;
        for i in 0..self.task_count {
            if self.tcbs[i].state != TaskState::Dead {
                count += 1;
            }
        }
        count
    }
}
