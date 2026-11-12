// LevelTicks and LevelChunkTicks: scheduled ticks kept per chunk, each
// chunk's queue in DRAIN_ORDER (trigger tick, priority, sub-tick order),
// drained across ticking chunks by INTRA_TICK_DRAIN_ORDER of their next
// tick (priority, sub-tick order), as Minecraft drains them.

use crate::dir::Pos;
use crate::fast::{FastMap, FastSet};
use std::collections::{BTreeSet, VecDeque};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Tick<T: Copy + Eq + std::hash::Hash> {
    pub pos: Pos,
    pub kind: T,
    pub trigger: i64,
    pub priority: i32,
    pub sub: i64,
}

// A chunk's queue (LevelChunkTicks.tickQueue, a java.util.PriorityQueue in
// DRAIN_ORDER: trigger tick, priority, sub-tick order), kept as the binary
// heap Java keeps, in the same array order (a saved chunk lists its ticks in
// that order), and the (pos, type) pairs it holds.
struct Container<T: Copy + Eq + std::hash::Hash> {
    queue: Vec<Tick<T>>,
    positions: FastSet<(Pos, T)>,
}

// ScheduledTick.DRAIN_ORDER.
fn drain_order<T: Copy + Eq + std::hash::Hash>(a: &Tick<T>, b: &Tick<T>) -> std::cmp::Ordering {
    (a.trigger, a.priority, a.sub).cmp(&(b.trigger, b.priority, b.sub))
}

impl<T: Copy + Eq + std::hash::Hash> Container<T> {
    fn new() -> Container<T> {
        Container { queue: Vec::new(), positions: FastSet::default() }
    }

    fn peek(&self) -> Option<Tick<T>> {
        self.queue.first().copied()
    }

    // PriorityQueue.offer.
    fn add(&mut self, tick: Tick<T>) {
        let at = self.queue.len();
        self.queue.push(tick);
        self.sift_up(at, tick);
    }

    // PriorityQueue.siftUp.
    fn sift_up(&mut self, mut k: usize, x: Tick<T>) {
        while k > 0 {
            let parent = (k - 1) >> 1;
            let e = self.queue[parent];
            if drain_order(&x, &e).is_ge() {
                break;
            }
            self.queue[k] = e;
            k = parent;
        }
        self.queue[k] = x;
    }

    // PriorityQueue.siftDown, over the first `n` elements.
    fn sift_down(&mut self, mut k: usize, x: Tick<T>, n: usize) {
        let half = n >> 1;
        while k < half {
            let mut child = 2 * k + 1;
            let mut c = self.queue[child];
            let right = child + 1;
            if right < n && drain_order(&c, &self.queue[right]).is_gt() {
                child = right;
                c = self.queue[child];
            }
            if drain_order(&x, &c).is_le() {
                break;
            }
            self.queue[k] = c;
            k = child;
        }
        self.queue[k] = x;
    }

    // PriorityQueue.poll.
    fn poll_heap(&mut self) -> Option<Tick<T>> {
        if self.queue.is_empty() {
            return None;
        }
        let result = self.queue[0];
        let last = self.queue.pop().unwrap();
        let n = self.queue.len();
        if n > 0 {
            self.sift_down(0, last, n);
        }
        Some(result)
    }

    // PriorityQueue.removeAt: the element moved ahead of the index, if any.
    fn remove_at(&mut self, i: usize) -> Option<Tick<T>> {
        let s = self.queue.len() - 1;
        if s == i {
            self.queue.pop();
            return None;
        }
        let moved = self.queue.pop().unwrap();
        self.sift_down(i, moved, s);
        if self.queue[i] == moved {
            self.sift_up(i, moved);
            if self.queue[i] != moved {
                return Some(moved);
            }
        }
        None
    }

    fn poll(&mut self) -> Option<Tick<T>> {
        let tick = self.poll_heap()?;
        self.positions.remove(&(tick.pos, tick.kind));
        Some(tick)
    }

    // LevelChunkTicks.removeIf: the queue's iterator removes what matches,
    // with PriorityQueue.Itr's handling of an element moved ahead of the cursor.
    fn remove_if(&mut self, predicate: impl Fn(&Tick<T>) -> bool) {
        let mut cursor = 0usize;
        let mut forget_me_not: VecDeque<Tick<T>> = VecDeque::new();
        // lastRet is an index in the array, or -1 with lastRetElt set.
        loop {
            let last_ret: Option<usize>;
            let element: Tick<T>;
            let mut last_ret_elt: Option<Tick<T>> = None;
            if cursor < self.queue.len() {
                last_ret = Some(cursor);
                element = self.queue[cursor];
                cursor += 1;
            } else if let Some(front) = forget_me_not.pop_front() {
                last_ret = None;
                element = front;
                last_ret_elt = Some(front);
            } else {
                break;
            }
            if predicate(&element) {
                // Itr.remove, then LevelChunkTicks removes the pair.
                if let Some(at) = last_ret {
                    match self.remove_at(at) {
                        None => cursor -= 1,
                        Some(moved) => forget_me_not.push_back(moved),
                    }
                } else if let Some(elt) = last_ret_elt {
                    // removeEq.
                    if let Some(at) = self.queue.iter().position(|t| *t == elt) {
                        self.remove_at(at);
                    }
                }
                self.positions.remove(&(element.pos, element.kind));
            }
        }
    }
}

pub struct LevelTicks<T: Copy + Eq + std::hash::Hash> {
    containers: FastMap<i64, Container<T>>,
    next_for_container: FastMap<i64, i64>,
    // The ticks collected to run this tick, in order, and those of them not
    // run yet (willTickThisTick).
    to_run: VecDeque<Tick<T>>,
    to_run_set: FastSet<(Pos, T)>,
}

impl<T: Copy + Eq + std::hash::Hash> LevelTicks<T> {
    pub fn new() -> LevelTicks<T> {
        LevelTicks { containers: FastMap::default(), next_for_container: FastMap::default(), to_run: VecDeque::new(), to_run_set: FastSet::default() }
    }

    // A new chunk's empty queue (LevelChunk.registerTickContainerInLevel).
    pub fn add_container(&mut self, chunk: i64) {
        self.containers.insert(chunk, Container::new());
    }

    // LevelTicks.schedule: dropped in a chunk not loaded; a tick for a
    // position and type already queued is not added again.
    pub fn schedule(&mut self, tick: Tick<T>) {
        let chunk = tick.pos.chunk();
        let Some(container) = self.containers.get_mut(&chunk) else { return };
        if !container.positions.insert((tick.pos, tick.kind)) {
            return;
        }
        container.add(tick);
        // The chunk schedule updater: a tick that is now the chunk's first
        // sets when the chunk next ticks.
        if container.peek().map(|first| first.sub) == Some(tick.sub) {
            self.next_for_container.insert(chunk, tick.trigger);
        }
    }

    // Whether the chunk's queue holds a tick of this sub-tick order.
    pub fn has_order(&self, chunk: i64, sub: i64) -> bool {
        self.containers.get(&chunk).map_or(false, |c| c.queue.iter().any(|tick| tick.sub == sub))
    }

    // LevelTicks.clearArea for the box from `min` to `max` (both inside):
    // the queued ticks at positions in it go, and so do those collected to
    // run this tick.
    pub fn clear_area(&mut self, min: Pos, max: Pos) {
        for cx in min.x() >> 4..=max.x() >> 4 {
            for cz in min.z() >> 4..=max.z() >> 4 {
                let chunk = crate::dir::chunk_key(cx, cz);
                let Some(container) = self.containers.get_mut(&chunk) else { continue };
                let first = container.peek().map(|tick| tick.sub);
                container.remove_if(|tick| inside_box(tick.pos, min, max));
                let now = container.peek();
                if now.map(|tick| tick.sub) != first {
                    match now {
                        Some(tick) => {
                            self.next_for_container.insert(chunk, tick.trigger);
                        }
                        None => {
                            self.next_for_container.remove(&chunk);
                        }
                    }
                }
            }
        }
        self.to_run.retain(|tick| !inside_box(tick.pos, min, max));
        self.to_run_set.retain(|(pos, _)| !inside_box(*pos, min, max));
    }

    // The tick queued for a position and type, if any (LevelChunkTicks.getAll
    // filtered by both).
    pub fn queued(&self, pos: Pos, kind: T) -> Option<Tick<T>> {
        self.containers.get(&pos.chunk())?.queue.iter().find(|t| t.pos == pos && t.kind == kind).copied()
    }

    pub fn has_scheduled_tick(&self, pos: Pos, kind: T) -> bool {
        self.containers.get(&pos.chunk()).map_or(false, |c| c.positions.contains(&(pos, kind)))
    }

    pub fn will_tick_this_tick(&self, pos: Pos, kind: T) -> bool {
        self.to_run_set.contains(&(pos, kind))
    }

    // Every queued tick (the chunks' queues), by sub-tick order.
    pub fn all(&self) -> Vec<Tick<T>> {
        let mut all: Vec<Tick<T>> = self.containers.values().flat_map(|c| c.queue.iter().copied()).collect();
        all.sort_by_key(|tick| tick.sub);
        all
    }

    pub fn is_empty(&self) -> bool {
        self.containers.values().all(|c| c.queue.is_empty())
    }

    // LevelChunkTicks.pack: a chunk's queue in the order of its heap.
    pub fn pack(&self, chunk: i64) -> Vec<Tick<T>> {
        self.containers.get(&chunk).map_or(Vec::new(), |c| c.queue.clone())
    }

    // LevelTicks.collectTicks: the ticks to run at `game_time`, at most
    // `max` of them, from chunks `ticking` accepts.
    pub fn collect(&mut self, game_time: i64, max: usize, ticking: impl Fn(i64) -> bool) {
        // sortContainersToTick.
        let mut to_tick: BTreeSet<(i32, i64, i64)> = BTreeSet::new();
        let mut remove = Vec::new();
        for (&chunk, next) in self.next_for_container.iter_mut() {
            if *next > game_time {
                continue;
            }
            let Some(container) = self.containers.get(&chunk) else {
                remove.push(chunk);
                continue;
            };
            match container.peek() {
                None => remove.push(chunk),
                Some(first) if first.trigger > game_time => *next = first.trigger,
                Some(first) => {
                    if ticking(chunk) {
                        remove.push(chunk);
                        to_tick.insert((first.priority, first.sub, chunk));
                    }
                }
            }
        }
        for chunk in remove {
            self.next_for_container.remove(&chunk);
        }
        // drainContainers.
        while self.to_run.len() < max {
            let Some(&entry) = to_tick.iter().next() else { break };
            to_tick.remove(&entry);
            let chunk = entry.2;
            let container = self.containers.get_mut(&chunk).unwrap();
            let tick = container.poll().unwrap();
            self.to_run.push_back(tick);
            // drainFromCurrentContainer: while this chunk's next tick is due
            // and comes before the next chunk's.
            let other = to_tick.iter().next().map(|&(priority, sub, _)| (priority, sub));
            while self.to_run.len() < max {
                let Some(next) = container.peek() else { break };
                if next.trigger > game_time || other.map_or(false, |o| (next.priority, next.sub) > o) {
                    break;
                }
                container.poll();
                self.to_run.push_back(next);
            }
            if let Some(next) = container.peek() {
                if next.trigger <= game_time && self.to_run.len() < max {
                    to_tick.insert((next.priority, next.sub, chunk));
                } else {
                    self.next_for_container.insert(chunk, next.trigger);
                }
            }
        }
        // rescheduleLeftoverContainers.
        for &(_, _, chunk) in &to_tick {
            let first = self.containers[&chunk].peek().unwrap();
            self.next_for_container.insert(chunk, first.trigger);
        }
        for tick in &self.to_run {
            self.to_run_set.insert((tick.pos, tick.kind));
        }
    }

    // The next collected tick to run (runCollectedTicks), or none: the
    // tick set is cleared after the last (cleanupAfterTick).
    pub fn next_to_run(&mut self) -> Option<Tick<T>> {
        let tick = self.to_run.pop_front();
        match tick {
            Some(tick) => {
                self.to_run_set.remove(&(tick.pos, tick.kind));
            }
            None => self.to_run_set.clear(),
        }
        tick
    }
}

fn inside_box(pos: Pos, min: Pos, max: Pos) -> bool {
    (min.x()..=max.x()).contains(&pos.x()) && (min.y()..=max.y()).contains(&pos.y()) && (min.z()..=max.z()).contains(&pos.z())
}
