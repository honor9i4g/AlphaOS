use alphaos_syscall_abi::InputEvent;
use alphaos_syscall_abi::{INPUT_EVENT_SYN, INPUT_SYN_DROPPED};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const CAPACITY: usize = 128;
const MASK: usize = CAPACITY - 1;

struct EventQueue {
    slots: [AtomicU64; CAPACITY],
    head: AtomicUsize,
    tail: AtomicUsize,
    dropped: AtomicU64,
}

impl EventQueue {
    const fn new() -> Self {
        Self {
            slots: [const { AtomicU64::new(0) }; CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
        }
    }

    fn push(&self, event: InputEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) & MASK;
        if next == self.head.load(Ordering::Acquire) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        let packed = u64::from(event.event_type)
            | (u64::from(event.code) << 16)
            | (u64::from(event.value as u8) << 32)
            | (u64::from(event.modifiers as u8) << 40);
        self.slots[tail].store(packed, Ordering::Relaxed);
        self.tail.store(next, Ordering::Release);
        true
    }

    fn pop(&self) -> Option<InputEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            let dropped = self.dropped.swap(0, Ordering::AcqRel);
            return (dropped != 0).then_some(InputEvent {
                event_type: INPUT_EVENT_SYN,
                code: INPUT_SYN_DROPPED,
                value: dropped.min(i32::MAX as u64) as i32,
                modifiers: 0,
                reserved: 0,
            });
        }

        let packed = self.slots[head].load(Ordering::Relaxed);
        self.head.store((head + 1) & MASK, Ordering::Release);

        Some(InputEvent {
            event_type: packed as u16,
            code: (packed >> 16) as u16,
            value: ((packed >> 32) as u8 as i8) as i32,
            modifiers: (packed >> 40) as u8 as u32,
            reserved: 0,
        })
    }
}

static EVENTS: EventQueue = EventQueue::new();
static FOREGROUND_TARGET: AtomicU64 = AtomicU64::new(0);

pub fn enqueue(event: InputEvent) -> bool {
    EVENTS.push(event)
}

pub fn set_foreground_target(target: Option<u64>) {
    FOREGROUND_TARGET.store(target.unwrap_or(0), Ordering::Release);
}

pub fn foreground_target() -> Option<u64> {
    match FOREGROUND_TARGET.load(Ordering::Acquire) {
        0 => None,
        target => Some(target),
    }
}

pub fn dequeue_for(target: u64) -> Option<InputEvent> {
    if target == 0 || FOREGROUND_TARGET.load(Ordering::Acquire) != target {
        return None;
    }
    EVENTS.pop()
}
