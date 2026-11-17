//! Messages to widgets: immediate and delayed, delivered by `Ui::update`.
//!
//! TB: tb_msg.cpp (TBMessageHandler). Only widgets receive messages; entries name
//! their target by `WidgetId` (H-01, H-03).

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::arena::WidgetId;
use crate::id::TbId;
use crate::value::Value;

/// Milliseconds, as set by the host with `Ui::set_time`.
pub type Millis = u64;

#[derive(Clone, Debug)]
pub struct Message {
    pub id: TbId,
    pub data: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub target: WidgetId,
    pub msg: Message,
    pub fire_at: Millis,
    pub seq: u64,
}

#[derive(Default)]
pub(crate) struct MessageQueue {
    immediate: VecDeque<Entry>,
    /// Sorted by (fire_at, seq).
    delayed: Vec<Entry>,
    seq: u64,
}

impl MessageQueue {
    pub fn post(&mut self, target: WidgetId, msg: Message) {
        self.seq += 1;
        self.immediate.push_back(Entry { target, msg, fire_at: 0, seq: self.seq });
    }

    pub fn post_at(&mut self, target: WidgetId, msg: Message, at: Millis) {
        self.seq += 1;
        let e = Entry { target, msg, fire_at: at, seq: self.seq };
        let pos = self.delayed.partition_point(|x| (x.fire_at, x.seq) <= (at, e.seq));
        self.delayed.insert(pos, e);
    }

    /// Remove messages for `target` (all, or only `id`). TB: DeleteMessage / DeleteAllMessages
    pub fn cancel(&mut self, target: WidgetId, id: Option<TbId>) {
        let keep = |e: &Entry| !(e.target == target && id.is_none_or(|i| e.msg.id == i));
        self.immediate.retain(keep);
        self.delayed.retain(keep);
    }

    pub fn cancel_where(&mut self, mut f: impl FnMut(WidgetId) -> bool) {
        self.immediate.retain(|e| !f(e.target));
        self.delayed.retain(|e| !f(e.target));
    }

    pub fn has(&self, target: WidgetId, id: TbId) -> bool {
        self.immediate.iter().chain(self.delayed.iter()).any(|e| e.target == target && e.msg.id == id)
    }

    /// Take everything due at `now`: delayed first (in fire order), then immediate.
    /// Messages posted while these are delivered wait for the next call.
    pub fn take_due(&mut self, now: Millis, out: &mut Vec<Entry>) {
        let n = self.delayed.partition_point(|e| e.fire_at <= now);
        out.extend(self.delayed.drain(..n));
        out.extend(self.immediate.drain(..));
    }

    /// Earliest time something is due: `Some(0)` if immediate messages are queued.
    pub fn next_due(&self) -> Option<Millis> {
        if !self.immediate.is_empty() {
            return Some(0);
        }
        self.delayed.first().map(|e| e.fire_at)
    }
}
