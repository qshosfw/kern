//! Microkernel Task & Actor Scheduler
//!
//! Provides strict cooperative run-to-completion task execution with priority classes.
//! Concurrency is temporal rather than spatial: tasks run on a shared stack, returning
//! quickly to yield control to the scheduler.

use crate::event::Event;

/// Scheduling Priority Band
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Background telemetry, housekeeping, diagnostics
    Idle = 0,
    /// User interface and rendering tasks
    Ui = 1,
    /// Radio state machine, squelch, tone detection
    Radio = 2,
    /// Packet capture, high-speed audio, DSP buffers
    Capture = 3,
    /// Emergency power cutoff, thermal/battery safety
    Safety = 4,
}

/// Cooperative Run-to-Completion Task State
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Yielded until next scheduled cycle or event
    Yielded,
    /// Waiting for specified tick count before next activation
    Wait(u32),
    /// Deferred to end of current cycle
    Deferred,
    /// Recoverable fault raised by actor with error code
    Fault(u8),
    /// Completed or terminated
    Terminated,
}

/// Demonomorphized actor step function pointer.
/// Signature: `fn(actor_instance_ptr: *mut (), ctx_ptr: *mut (), event: Option<&Event>) -> TaskStatus`
pub type ActorStepFn = unsafe fn(*mut (), *mut (), Option<&Event>) -> TaskStatus;

/// Zero-allocation Actor / Coroutine Trait
pub trait Actor<C> {
    fn name(&self) -> &'static str;
    fn priority(&self) -> Priority {
        Priority::Ui
    }
    /// Run-to-completion step invocation
    fn step(&mut self, ctx: &mut C, event: Option<&Event>) -> TaskStatus;
}

/// Actor Control Block (TCB)
pub struct ActorBlock<'a, C> {
    pub instance_ptr: Option<*mut ()>,
    pub step_fn: Option<ActorStepFn>,
    pub priority: Priority,
    pub status: TaskStatus,
    pub wait_counter: u32,
    _marker: core::marker::PhantomData<(&'a (), C)>,
}

impl<'a, C> ActorBlock<'a, C> {
    pub const fn empty() -> Self {
        Self {
            instance_ptr: None,
            step_fn: None,
            priority: Priority::Idle,
            status: TaskStatus::Terminated,
            wait_counter: 0,
            _marker: core::marker::PhantomData,
        }
    }
}

/// Static, zero-allocation Run-to-Completion Scheduler
pub struct Scheduler<'a, C, const N: usize = 8> {
    pub actors: [ActorBlock<'a, C>; N],
    count: usize,
}

impl<'a, C, const N: usize> Default for Scheduler<'a, C, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, C, const N: usize> Scheduler<'a, C, N> {
    pub const fn new() -> Self {
        Self {
            actors: [const { ActorBlock::empty() }; N],
            count: 0,
        }
    }

    /// Register a typed actor into the scheduler using function-pointer bridging
    pub fn register<A: Actor<C>>(&mut self, actor: &'a mut A) -> Result<(), ()> {
        if self.count >= N {
            return Err(());
        }

        unsafe fn step_thunk<A: Actor<C>, C>(
            actor_ptr: *mut (),
            ctx_ptr: *mut (),
            event: Option<&Event>,
        ) -> TaskStatus {
            let act = unsafe { &mut *(actor_ptr as *mut A) };
            let ctx = unsafe { &mut *(ctx_ptr as *mut C) };
            act.step(ctx, event)
        }

        let prio = actor.priority();
        self.actors[self.count] = ActorBlock {
            instance_ptr: Some(actor as *mut A as *mut ()),
            step_fn: Some(step_thunk::<A, C>),
            priority: prio,
            status: TaskStatus::Yielded,
            wait_counter: 0,
            _marker: core::marker::PhantomData,
        };
        self.count += 1;
        Ok(())
    }

    /// Register an actor directly via function pointer and raw instance pointer (zero monomorphization)
    pub fn register_raw(
        &mut self,
        instance_ptr: *mut (),
        step_fn: ActorStepFn,
        priority: Priority,
    ) -> Result<(), ()> {
        if self.count >= N {
            return Err(());
        }
        self.actors[self.count] = ActorBlock {
            instance_ptr: Some(instance_ptr),
            step_fn: Some(step_fn),
            priority,
            status: TaskStatus::Yielded,
            wait_counter: 0,
            _marker: core::marker::PhantomData,
        };
        self.count += 1;
        Ok(())
    }

    /// Tick all active actors according to priority and temporal state
    pub fn dispatch(&mut self, ctx: &mut C, event: Option<&Event>) {
        let is_tick = matches!(event, Some(Event::Tick));

        // Iterate highest priority to lowest priority
        for prio_level in [
            Priority::Safety,
            Priority::Capture,
            Priority::Radio,
            Priority::Ui,
            Priority::Idle,
        ] {
            for i in 0..self.count {
                let block = &mut self.actors[i];
                if block.status == TaskStatus::Terminated
                    || matches!(block.status, TaskStatus::Fault(_))
                    || block.priority != prio_level
                {
                    continue;
                }

                // Handle waiting state
                if let TaskStatus::Wait(ref mut ticks) = block.status {
                    if is_tick {
                        if *ticks > 0 {
                            *ticks -= 1;
                        }
                        if *ticks == 0 {
                            block.status = TaskStatus::Yielded;
                        }
                    }
                    if block.status != TaskStatus::Yielded {
                        continue;
                    }
                }

                if block.status == TaskStatus::Deferred {
                    block.status = TaskStatus::Yielded;
                }

                if block.status == TaskStatus::Yielded {
                    if let (Some(inst_ptr), Some(step)) = (block.instance_ptr, block.step_fn) {
                        unsafe {
                            block.status = step(inst_ptr, ctx as *mut C as *mut (), event);
                        }
                    }
                }
            }
        }
    }

    /// Count of currently active actors
    pub fn active_count(&self) -> usize {
        self.actors
            .iter()
            .take(self.count)
            .filter(|a| {
                a.status != TaskStatus::Terminated && !matches!(a.status, TaskStatus::Fault(_))
            })
            .count()
    }
}
