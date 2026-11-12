// Java collections whose iteration order shows in what the engine writes: a
// java.util.HashSet<BlockPos> and the key layout of fastutil's
// Object2ObjectOpenHashMap<BlockPos, _>.

use crate::dir::Pos;

// java.util.HashMap<BlockPos, V>, ported from JDK 25's HashMap.java with its
// tree bins, and the java.util.HashSet<BlockPos> on top of it. Iteration is
// the table in index order, each bin in the order of its `next` links (for a
// tree bin that starts at the root moveRootToFront put first).
//
// BlockPos declares no interfaces of its own (only Vec3i implements
// Comparable<Vec3i>), so comparableClassFor(BlockPos) is null: TreeNode
// ordering is the hash and then tieBreakOrder, and compareComparables never
// runs. Two distinct keys with one hash and one class (every key here is an
// immutable BlockPos) are ordered by System.identityHashCode, which no port
// can reproduce: that is the one refusal, except in a set whose order shows
// in nothing the engine writes (JavaPosSet::order_free).

const NIL: u32 = u32::MAX;
const TREEIFY_THRESHOLD: usize = 8;
const UNTREEIFY_THRESHOLD: usize = 6;
const MIN_TREEIFY_CAPACITY: usize = 64;

struct Node<V> {
    hash: i32,
    key: Pos,
    val: V,
    next: u32,
    // The TreeNode fields; `tree` is whether this node is a TreeNode.
    tree: bool,
    parent: u32,
    left: u32,
    right: u32,
    prev: u32,
    red: bool,
}

// HashMap.hash(Vec3i.hashCode()): (y + z * 31) * 31 + x, spread.
fn hash(pos: Pos) -> i32 {
    let h = pos.y().wrapping_add(pos.z().wrapping_mul(31)).wrapping_mul(31).wrapping_add(pos.x());
    h ^ ((h as u32) >> 16) as i32
}

// TreeNode.tieBreakOrder between two BlockPos of one class.
fn identity_tie(_: Pos, _: Pos) -> i32 {
    crate::level::refuse("a java.util.HashMap tree bin ordered two BlockPos of one hash by System.identityHashCode")
}

pub struct JavaHashMap<V> {
    nodes: Vec<Node<V>>,
    table: Vec<u32>,
    size: usize,
    threshold: usize,
    // tieBreakOrder(a, b) for keys of equal hash; tests give it class names.
    tie_break: fn(Pos, Pos) -> i32,
}

macro_rules! n {
    ($s:expr, $i:expr) => {
        $s.nodes[$i as usize]
    };
}

impl<V: Copy> JavaHashMap<V> {
    pub fn new() -> JavaHashMap<V> {
        JavaHashMap { nodes: Vec::new(), table: Vec::new(), size: 0, threshold: 0, tie_break: identity_tie }
    }

    pub fn with_tie_break(tie_break: fn(Pos, Pos) -> i32) -> JavaHashMap<V> {
        JavaHashMap { nodes: Vec::new(), table: Vec::new(), size: 0, threshold: 0, tie_break }
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    fn new_node(&mut self, hash: i32, key: Pos, val: V, next: u32, tree: bool) -> u32 {
        self.nodes.push(Node { hash, key, val, next, tree, parent: NIL, left: NIL, right: NIL, prev: NIL, red: false });
        (self.nodes.len() - 1) as u32
    }

    // HashMap.put (and HashSet.add, whose value never matters).
    pub fn put(&mut self, key: Pos, val: V) {
        let h = hash(key);
        if self.table.is_empty() {
            self.resize();
        }
        let i = (self.table.len() - 1) & h as u32 as usize;
        let p = self.table[i];
        if p == NIL {
            self.table[i] = self.new_node(h, key, val, NIL, false);
        } else {
            let mut e = NIL;
            if n!(self, p).hash == h && n!(self, p).key == key {
                e = p;
            } else if n!(self, p).tree {
                e = self.put_tree_val(p, h, key, val);
            } else {
                let mut p = p;
                let mut bin_count = 0;
                loop {
                    let next = n!(self, p).next;
                    if next == NIL {
                        n!(self, p).next = self.new_node(h, key, val, NIL, false);
                        if bin_count >= TREEIFY_THRESHOLD - 1 {
                            self.treeify_bin(h);
                        }
                        break;
                    }
                    if n!(self, next).hash == h && n!(self, next).key == key {
                        e = next;
                        break;
                    }
                    p = next;
                    bin_count += 1;
                }
            }
            if e != NIL {
                n!(self, e).val = val;
                return;
            }
        }
        self.size += 1;
        if self.size > self.threshold {
            self.resize();
        }
    }

    // HashMap.replace(key, value): only a key the map holds.
    pub fn replace(&mut self, key: Pos, val: V) {
        let node = self.get_node(key);
        if node != NIL {
            n!(self, node).val = val;
        }
    }

    // HashMap.getNode.
    fn get_node(&self, key: Pos) -> u32 {
        self.find_node(hash(key), key).map_or(NIL, |(node, _)| node)
    }

    // removeNode's lookup: the node, and its predecessor in a plain bin.
    fn find_node(&self, h: i32, key: Pos) -> Option<(u32, u32)> {
        if self.table.is_empty() {
            return None;
        }
        let first = self.table[(self.table.len() - 1) & h as u32 as usize];
        if first == NIL {
            return None;
        }
        if n!(self, first).hash == h && n!(self, first).key == key {
            return Some((first, first));
        }
        let mut p = first;
        let mut e = n!(self, first).next;
        if e == NIL {
            return None;
        }
        if n!(self, p).tree {
            let node = self.root(p);
            let found = self.find(node, h, key);
            return if found == NIL { None } else { Some((found, p)) };
        }
        while e != NIL {
            if n!(self, e).hash == h && n!(self, e).key == key {
                return Some((e, p));
            }
            p = e;
            e = n!(self, e).next;
        }
        None
    }

    // HashMap.remove(key).
    pub fn remove(&mut self, key: Pos) {
        let h = hash(key);
        let Some((node, p)) = self.find_node(h, key) else { return };
        let index = (self.table.len() - 1) & h as u32 as usize;
        if n!(self, node).tree {
            self.remove_tree_node(node);
        } else if node == p {
            self.table[index] = n!(self, node).next;
        } else {
            n!(self, p).next = n!(self, node).next;
        }
        self.size -= 1;
    }

    // The entries in iteration order.
    pub fn entries(&self) -> Vec<(Pos, V)> {
        let mut out = Vec::with_capacity(self.size);
        for &head in &self.table {
            let mut e = head;
            while e != NIL {
                out.push((n!(self, e).key, n!(self, e).val));
                e = n!(self, e).next;
            }
        }
        out
    }

    // HashMap.resize, for the default capacity and load factor.
    fn resize(&mut self) {
        let old_cap = self.table.len();
        let new_cap = if old_cap == 0 { 16 } else { old_cap * 2 };
        self.threshold = new_cap * 3 / 4;
        let old_tab = std::mem::replace(&mut self.table, vec![NIL; new_cap]);
        for (j, &head) in old_tab.iter().enumerate() {
            let mut e = head;
            if e == NIL {
                continue;
            }
            if n!(self, e).next == NIL {
                let at = n!(self, e).hash as u32 as usize & (new_cap - 1);
                self.table[at] = e;
            } else if n!(self, e).tree {
                self.split(e, j, old_cap);
            } else {
                let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
                while e != NIL {
                    let next = n!(self, e).next;
                    if n!(self, e).hash as u32 as usize & old_cap == 0 {
                        if lo_tail == NIL {
                            lo_head = e;
                        } else {
                            n!(self, lo_tail).next = e;
                        }
                        lo_tail = e;
                    } else {
                        if hi_tail == NIL {
                            hi_head = e;
                        } else {
                            n!(self, hi_tail).next = e;
                        }
                        hi_tail = e;
                    }
                    e = next;
                }
                if lo_tail != NIL {
                    n!(self, lo_tail).next = NIL;
                    self.table[j] = lo_head;
                }
                if hi_tail != NIL {
                    n!(self, hi_tail).next = NIL;
                    self.table[j + old_cap] = hi_head;
                }
            }
        }
    }

    // HashMap.treeifyBin: the bin of this hash becomes a tree, or the table
    // doubles while it is under MIN_TREEIFY_CAPACITY.
    fn treeify_bin(&mut self, h: i32) {
        if self.table.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
            return;
        }
        let index = (self.table.len() - 1) & h as u32 as usize;
        let head = self.table[index];
        if head == NIL {
            return;
        }
        // replacementTreeNode for each node, linked in order with prev.
        let mut tl = NIL;
        let mut e = head;
        while e != NIL {
            let node = &mut n!(self, e);
            node.tree = true;
            node.parent = NIL;
            node.left = NIL;
            node.right = NIL;
            node.red = false;
            node.prev = tl;
            tl = e;
            e = n!(self, e).next;
        }
        self.treeify(head);
    }

    // TreeNode.root.
    fn root(&self, mut r: u32) -> u32 {
        while n!(self, r).parent != NIL {
            r = n!(self, r).parent;
        }
        r
    }

    // TreeNode.moveRootToFront.
    fn move_root_to_front(&mut self, root: u32) {
        if root == NIL || self.table.is_empty() {
            return;
        }
        let index = (self.table.len() - 1) & n!(self, root).hash as u32 as usize;
        let first = self.table[index];
        if root != first {
            self.table[index] = root;
            let rp = n!(self, root).prev;
            let rn = n!(self, root).next;
            if rn != NIL {
                n!(self, rn).prev = rp;
            }
            if rp != NIL {
                n!(self, rp).next = rn;
            }
            if first != NIL {
                n!(self, first).prev = root;
            }
            n!(self, root).next = first;
            n!(self, root).prev = NIL;
        }
    }

    // TreeNode.find with kc == null (comparableClassFor(BlockPos) is null,
    // so it never compares and searches both children when the hash ties).
    fn find(&self, mut p: u32, h: i32, k: Pos) -> u32 {
        loop {
            let (pl, pr) = (n!(self, p).left, n!(self, p).right);
            let ph = n!(self, p).hash;
            if ph > h {
                p = pl;
            } else if ph < h {
                p = pr;
            } else if n!(self, p).key == k {
                return p;
            } else if pl == NIL {
                p = pr;
            } else if pr == NIL {
                p = pl;
            } else {
                let q = self.find(pr, h, k);
                if q != NIL {
                    return q;
                }
                p = pl;
            }
            if p == NIL {
                return NIL;
            }
        }
    }

    // TreeNode.treeify: forms a tree of the nodes linked from `head`.
    fn treeify(&mut self, head: u32) {
        let mut root = NIL;
        let mut x = head;
        while x != NIL {
            let next = n!(self, x).next;
            n!(self, x).left = NIL;
            n!(self, x).right = NIL;
            if root == NIL {
                n!(self, x).parent = NIL;
                n!(self, x).red = false;
                root = x;
            } else {
                let k = n!(self, x).key;
                let h = n!(self, x).hash;
                let mut p = root;
                loop {
                    let pk = n!(self, p).key;
                    let ph = n!(self, p).hash;
                    let dir = if ph > h {
                        -1
                    } else if ph < h {
                        1
                    } else {
                        (self.tie_break)(k, pk)
                    };
                    let xp = p;
                    p = if dir <= 0 { n!(self, xp).left } else { n!(self, xp).right };
                    if p == NIL {
                        n!(self, x).parent = xp;
                        if dir <= 0 {
                            n!(self, xp).left = x;
                        } else {
                            n!(self, xp).right = x;
                        }
                        root = self.balance_insertion(root, x);
                        break;
                    }
                }
            }
            x = next;
        }
        self.move_root_to_front(root);
    }

    // TreeNode.untreeify: the nodes linked from `head` become plain nodes.
    fn untreeify(&mut self, head: u32) -> u32 {
        let mut q = head;
        while q != NIL {
            let node = &mut n!(self, q);
            node.tree = false;
            node.parent = NIL;
            node.left = NIL;
            node.right = NIL;
            node.prev = NIL;
            q = n!(self, q).next;
        }
        head
    }

    // TreeNode.putTreeVal: the node of an existing key, or NIL after
    // inserting one.
    fn put_tree_val(&mut self, first: u32, h: i32, k: Pos, v: V) -> u32 {
        let mut searched = false;
        let root = if n!(self, first).parent != NIL { self.root(first) } else { first };
        let mut p = root;
        loop {
            let ph = n!(self, p).hash;
            let dir;
            if ph > h {
                dir = -1;
            } else if ph < h {
                dir = 1;
            } else if n!(self, p).key == k {
                return p;
            } else {
                if !searched {
                    searched = true;
                    let ch = n!(self, p).left;
                    if ch != NIL {
                        let q = self.find(ch, h, k);
                        if q != NIL {
                            return q;
                        }
                    }
                    let ch = n!(self, p).right;
                    if ch != NIL {
                        let q = self.find(ch, h, k);
                        if q != NIL {
                            return q;
                        }
                    }
                }
                dir = (self.tie_break)(k, n!(self, p).key);
            }
            let xp = p;
            p = if dir <= 0 { n!(self, xp).left } else { n!(self, xp).right };
            if p == NIL {
                let xpn = n!(self, xp).next;
                let x = self.new_node(h, k, v, xpn, true);
                if dir <= 0 {
                    n!(self, xp).left = x;
                } else {
                    n!(self, xp).right = x;
                }
                n!(self, xp).next = x;
                n!(self, x).parent = xp;
                n!(self, x).prev = xp;
                if xpn != NIL {
                    n!(self, xpn).prev = x;
                }
                let r = self.balance_insertion(root, x);
                self.move_root_to_front(r);
                return NIL;
            }
        }
    }

    // TreeNode.removeTreeNode with movable = true (HashMap.remove).
    fn remove_tree_node(&mut self, this: u32) {
        let index = (self.table.len() - 1) & n!(self, this).hash as u32 as usize;
        let mut first = self.table[index];
        let mut root = first;
        let succ = n!(self, this).next;
        let pred = n!(self, this).prev;
        if pred == NIL {
            first = succ;
            self.table[index] = succ;
        } else {
            n!(self, pred).next = succ;
        }
        if succ != NIL {
            n!(self, succ).prev = pred;
        }
        if first == NIL {
            return;
        }
        if n!(self, root).parent != NIL {
            root = self.root(root);
        }
        let too_small = root == NIL || n!(self, root).right == NIL || {
            let rl = n!(self, root).left;
            rl == NIL || n!(self, rl).left == NIL
        };
        if too_small {
            self.table[index] = self.untreeify(first);
            return;
        }
        let p = this;
        let (pl, pr) = (n!(self, p).left, n!(self, p).right);
        let replacement;
        if pl != NIL && pr != NIL {
            let mut s = pr;
            while n!(self, s).left != NIL {
                s = n!(self, s).left;
            }
            let c = n!(self, s).red;
            n!(self, s).red = n!(self, p).red;
            n!(self, p).red = c;
            let sr = n!(self, s).right;
            let pp = n!(self, p).parent;
            if s == pr {
                n!(self, p).parent = s;
                n!(self, s).right = p;
            } else {
                let sp = n!(self, s).parent;
                n!(self, p).parent = sp;
                if sp != NIL {
                    if s == n!(self, sp).left {
                        n!(self, sp).left = p;
                    } else {
                        n!(self, sp).right = p;
                    }
                }
                n!(self, s).right = pr;
                if pr != NIL {
                    n!(self, pr).parent = s;
                }
            }
            n!(self, p).left = NIL;
            n!(self, p).right = sr;
            if sr != NIL {
                n!(self, sr).parent = p;
            }
            n!(self, s).left = pl;
            if pl != NIL {
                n!(self, pl).parent = s;
            }
            n!(self, s).parent = pp;
            if pp == NIL {
                root = s;
            } else if p == n!(self, pp).left {
                n!(self, pp).left = s;
            } else {
                n!(self, pp).right = s;
            }
            replacement = if sr != NIL { sr } else { p };
        } else if pl != NIL {
            replacement = pl;
        } else if pr != NIL {
            replacement = pr;
        } else {
            replacement = p;
        }
        if replacement != p {
            let pp = n!(self, p).parent;
            n!(self, replacement).parent = pp;
            if pp == NIL {
                root = replacement;
                n!(self, replacement).red = false;
            } else if p == n!(self, pp).left {
                n!(self, pp).left = replacement;
            } else {
                n!(self, pp).right = replacement;
            }
            n!(self, p).left = NIL;
            n!(self, p).right = NIL;
            n!(self, p).parent = NIL;
        }
        let r = if n!(self, p).red { root } else { self.balance_deletion(root, replacement) };
        if replacement == p {
            let pp = n!(self, p).parent;
            n!(self, p).parent = NIL;
            if pp != NIL {
                if p == n!(self, pp).left {
                    n!(self, pp).left = NIL;
                } else if p == n!(self, pp).right {
                    n!(self, pp).right = NIL;
                }
            }
        }
        self.move_root_to_front(r);
    }

    // TreeNode.split: a tree bin splits into lower and upper bins on resize,
    // or untreeifies when a half is small. `index` is the old bin and `bit`
    // the old capacity.
    fn split(&mut self, b: u32, index: usize, bit: usize) {
        let (mut lo_head, mut lo_tail, mut hi_head, mut hi_tail) = (NIL, NIL, NIL, NIL);
        let (mut lc, mut hc) = (0, 0);
        let mut e = b;
        while e != NIL {
            let next = n!(self, e).next;
            n!(self, e).next = NIL;
            if n!(self, e).hash as u32 as usize & bit == 0 {
                n!(self, e).prev = lo_tail;
                if lo_tail == NIL {
                    lo_head = e;
                } else {
                    n!(self, lo_tail).next = e;
                }
                lo_tail = e;
                lc += 1;
            } else {
                n!(self, e).prev = hi_tail;
                if hi_tail == NIL {
                    hi_head = e;
                } else {
                    n!(self, hi_tail).next = e;
                }
                hi_tail = e;
                hc += 1;
            }
            e = next;
        }
        if lo_head != NIL {
            if lc <= UNTREEIFY_THRESHOLD {
                self.table[index] = self.untreeify(lo_head);
            } else {
                self.table[index] = lo_head;
                if hi_head != NIL {
                    self.treeify(lo_head);
                }
            }
        }
        if hi_head != NIL {
            if hc <= UNTREEIFY_THRESHOLD {
                self.table[index + bit] = self.untreeify(hi_head);
            } else {
                self.table[index + bit] = hi_head;
                if lo_head != NIL {
                    self.treeify(hi_head);
                }
            }
        }
    }

    fn rotate_left(&mut self, mut root: u32, p: u32) -> u32 {
        if p != NIL {
            let r = n!(self, p).right;
            if r != NIL {
                let rl = n!(self, r).left;
                n!(self, p).right = rl;
                if rl != NIL {
                    n!(self, rl).parent = p;
                }
                let pp = n!(self, p).parent;
                n!(self, r).parent = pp;
                if pp == NIL {
                    root = r;
                    n!(self, r).red = false;
                } else if n!(self, pp).left == p {
                    n!(self, pp).left = r;
                } else {
                    n!(self, pp).right = r;
                }
                n!(self, r).left = p;
                n!(self, p).parent = r;
            }
        }
        root
    }

    fn rotate_right(&mut self, mut root: u32, p: u32) -> u32 {
        if p != NIL {
            let l = n!(self, p).left;
            if l != NIL {
                let lr = n!(self, l).right;
                n!(self, p).left = lr;
                if lr != NIL {
                    n!(self, lr).parent = p;
                }
                let pp = n!(self, p).parent;
                n!(self, l).parent = pp;
                if pp == NIL {
                    root = l;
                    n!(self, l).red = false;
                } else if n!(self, pp).right == p {
                    n!(self, pp).right = l;
                } else {
                    n!(self, pp).left = l;
                }
                n!(self, l).right = p;
                n!(self, p).parent = l;
            }
        }
        root
    }

    fn balance_insertion(&mut self, mut root: u32, mut x: u32) -> u32 {
        n!(self, x).red = true;
        loop {
            let mut xp = n!(self, x).parent;
            if xp == NIL {
                n!(self, x).red = false;
                return x;
            }
            if !n!(self, xp).red {
                return root;
            }
            let mut xpp = n!(self, xp).parent;
            if xpp == NIL {
                return root;
            }
            let xppl = n!(self, xpp).left;
            if xp == xppl {
                let xppr = n!(self, xpp).right;
                if xppr != NIL && n!(self, xppr).red {
                    n!(self, xppr).red = false;
                    n!(self, xp).red = false;
                    n!(self, xpp).red = true;
                    x = xpp;
                } else {
                    if x == n!(self, xp).right {
                        x = xp;
                        root = self.rotate_left(root, x);
                        xp = n!(self, x).parent;
                        xpp = if xp == NIL { NIL } else { n!(self, xp).parent };
                    }
                    if xp != NIL {
                        n!(self, xp).red = false;
                        if xpp != NIL {
                            n!(self, xpp).red = true;
                            root = self.rotate_right(root, xpp);
                        }
                    }
                }
            } else if xppl != NIL && n!(self, xppl).red {
                n!(self, xppl).red = false;
                n!(self, xp).red = false;
                n!(self, xpp).red = true;
                x = xpp;
            } else {
                if x == n!(self, xp).left {
                    x = xp;
                    root = self.rotate_right(root, x);
                    xp = n!(self, x).parent;
                    xpp = if xp == NIL { NIL } else { n!(self, xp).parent };
                }
                if xp != NIL {
                    n!(self, xp).red = false;
                    if xpp != NIL {
                        n!(self, xpp).red = true;
                        root = self.rotate_left(root, xpp);
                    }
                }
            }
        }
    }

    fn balance_deletion(&mut self, mut root: u32, mut x: u32) -> u32 {
        loop {
            if x == NIL || x == root {
                return root;
            }
            let mut xp = n!(self, x).parent;
            if xp == NIL {
                n!(self, x).red = false;
                return x;
            }
            if n!(self, x).red {
                n!(self, x).red = false;
                return root;
            }
            let xpl = n!(self, xp).left;
            if xpl == x {
                let mut xpr = n!(self, xp).right;
                if xpr != NIL && n!(self, xpr).red {
                    n!(self, xpr).red = false;
                    n!(self, xp).red = true;
                    root = self.rotate_left(root, xp);
                    xp = n!(self, x).parent;
                    xpr = if xp == NIL { NIL } else { n!(self, xp).right };
                }
                if xpr == NIL {
                    x = xp;
                } else {
                    let (sl, mut sr) = (n!(self, xpr).left, n!(self, xpr).right);
                    if (sr == NIL || !n!(self, sr).red) && (sl == NIL || !n!(self, sl).red) {
                        n!(self, xpr).red = true;
                        x = xp;
                    } else {
                        if sr == NIL || !n!(self, sr).red {
                            if sl != NIL {
                                n!(self, sl).red = false;
                            }
                            n!(self, xpr).red = true;
                            root = self.rotate_right(root, xpr);
                            xp = n!(self, x).parent;
                            xpr = if xp == NIL { NIL } else { n!(self, xp).right };
                        }
                        if xpr != NIL {
                            n!(self, xpr).red = if xp == NIL { false } else { n!(self, xp).red };
                            sr = n!(self, xpr).right;
                            if sr != NIL {
                                n!(self, sr).red = false;
                            }
                        }
                        if xp != NIL {
                            n!(self, xp).red = false;
                            root = self.rotate_left(root, xp);
                        }
                        x = root;
                    }
                }
            } else {
                let mut xpl = xpl;
                if xpl != NIL && n!(self, xpl).red {
                    n!(self, xpl).red = false;
                    n!(self, xp).red = true;
                    root = self.rotate_right(root, xp);
                    xp = n!(self, x).parent;
                    xpl = if xp == NIL { NIL } else { n!(self, xp).left };
                }
                if xpl == NIL {
                    x = xp;
                } else {
                    let (mut sl, sr) = (n!(self, xpl).left, n!(self, xpl).right);
                    if (sl == NIL || !n!(self, sl).red) && (sr == NIL || !n!(self, sr).red) {
                        n!(self, xpl).red = true;
                        x = xp;
                    } else {
                        if sl == NIL || !n!(self, sl).red {
                            if sr != NIL {
                                n!(self, sr).red = false;
                            }
                            n!(self, xpl).red = true;
                            root = self.rotate_left(root, xpl);
                            xp = n!(self, x).parent;
                            xpl = if xp == NIL { NIL } else { n!(self, xp).left };
                        }
                        if xpl != NIL {
                            n!(self, xpl).red = if xp == NIL { false } else { n!(self, xp).red };
                            sl = n!(self, xpl).left;
                            if sl != NIL {
                                n!(self, sl).red = false;
                            }
                        }
                        if xp != NIL {
                            n!(self, xp).red = false;
                            root = self.rotate_right(root, xp);
                        }
                        x = root;
                    }
                }
            }
        }
    }
}

// A java.util.HashSet<BlockPos>: a HashMap whose values do not matter.
pub struct JavaPosSet {
    map: JavaHashMap<()>,
}

// The tie two keys of one hash take in a set whose order shows in nothing
// the engine writes: by position, where Java's order is by identity hash.
fn position_tie(a: Pos, b: Pos) -> i32 {
    if (a.x(), a.y(), a.z()) < (b.x(), b.y(), b.z()) { -1 } else { 1 }
}

impl JavaPosSet {
    pub fn new() -> JavaPosSet {
        JavaPosSet { map: JavaHashMap::new() }
    }

    // A set iterated as Java's is, but where what the engine writes is the
    // same in any order of its keys: the identity-hash ties of its tree bins
    // are broken by position instead of refused. The light engine's pending
    // changes are one: the pinned server's own code, run with
    // java.util.HashMap's identity ties reversed and in two seeded random
    // orders (512 to 1,024 ties a run), wrote the same blueprints, patches,
    // entities and state (light, heightmaps, scheduled ticks) as with its
    // own order on the four worlds of 506,091 to 3,392,900 blocks where the
    // ties occur.
    pub fn order_free() -> JavaPosSet {
        JavaPosSet { map: JavaHashMap::with_tie_break(position_tie) }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn insert(&mut self, pos: Pos) {
        self.map.put(pos, ());
    }

    pub fn to_vec(&self) -> Vec<Pos> {
        self.map.entries().into_iter().map(|(pos, ())| pos).collect()
    }
}

// The keys of a fastutil Object2ObjectOpenHashMap<BlockPos, _> with the
// default size and load factor, laid out as it lays them: open addressing
// with linear probing, backward-shift deletion, growth and shrinking.
// Iteration runs from the end of the table backwards.
pub struct OpenHashKeys {
    key: Vec<Option<Pos>>,
    size: usize,
    min_n: usize,
    max_fill: usize,
}

const LOAD_FACTOR: f32 = 0.75;

// HashCommon.arraySize.
fn array_size(expected: usize) -> usize {
    let s = ((expected as f32 / LOAD_FACTOR).ceil() as u64).next_power_of_two().max(2);
    s as usize
}

// HashCommon.maxFill.
fn max_fill(n: usize) -> usize {
    (((n as f32) * LOAD_FACTOR).ceil() as usize).min(n - 1)
}

// HashCommon.mix(k.hashCode()).
fn mix(pos: Pos) -> u32 {
    let hash = pos.y().wrapping_add(pos.z().wrapping_mul(31)).wrapping_mul(31).wrapping_add(pos.x());
    let h = (hash as u32).wrapping_mul(0x9E3779B9);
    h ^ (h >> 16)
}

impl OpenHashKeys {
    pub fn new() -> OpenHashKeys {
        let n = array_size(16);
        OpenHashKeys { key: vec![None; n], size: 0, min_n: n, max_fill: max_fill(n) }
    }

    fn mask(&self) -> usize {
        self.key.len() - 1
    }

    fn find(&self, pos: Pos) -> Result<usize, usize> {
        let mask = self.mask();
        let mut at = mix(pos) as usize & mask;
        loop {
            match self.key[at] {
                None => return Err(at),
                Some(k) if k == pos => return Ok(at),
                Some(_) => at = (at + 1) & mask,
            }
        }
    }

    // put: a key that is there stays where it is.
    pub fn put(&mut self, pos: Pos) {
        if let Err(at) = self.find(pos) {
            self.key[at] = Some(pos);
            let before = self.size;
            self.size += 1;
            if before >= self.max_fill {
                self.rehash(array_size(self.size + 1));
            }
        }
    }

    fn rehash(&mut self, new_n: usize) {
        let mask = new_n - 1;
        let mut new_key: Vec<Option<Pos>> = vec![None; new_n];
        let mut i = self.key.len();
        for _ in 0..self.size {
            loop {
                i -= 1;
                if self.key[i].is_some() {
                    break;
                }
            }
            let k = self.key[i].unwrap();
            let mut at = mix(k) as usize & mask;
            while new_key[at].is_some() {
                at = (at + 1) & mask;
            }
            new_key[at] = Some(k);
        }
        self.key = new_key;
        self.max_fill = max_fill(new_n);
    }

    pub fn remove(&mut self, pos: Pos) {
        let Ok(at) = self.find(pos) else { return };
        self.size -= 1;
        self.shift_keys(at);
        let n = self.key.len();
        if n > self.min_n && self.size < self.max_fill / 4 && n > 16 {
            self.rehash(n / 2);
        }
    }

    fn shift_keys(&mut self, mut pos: usize) {
        let mask = self.mask();
        loop {
            let last = pos;
            pos = (last + 1) & mask;
            loop {
                let Some(curr) = self.key[pos] else {
                    self.key[last] = None;
                    return;
                };
                let slot = mix(curr) as usize & mask;
                let stays = if last <= pos { last >= slot || slot > pos } else { last >= slot && slot > pos };
                if stays {
                    break;
                }
                pos = (pos + 1) & mask;
            }
            self.key[last] = self.key[pos];
        }
    }

    // The keys in iteration order.
    pub fn iter(&self) -> impl Iterator<Item = Pos> + '_ {
        self.key.iter().rev().filter_map(|k| *k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static IDENTITY: Cell<bool> = const { Cell::new(false) };
    }

    // Mode S: every key is one class, so any tie of hash is an identity tie.
    fn tie_same_class(_: Pos, _: Pos) -> i32 {
        IDENTITY.with(|f| f.set(true));
        -1
    }

    // Mode M: a key is PosA when z is even and PosB when it is odd; classes
    // order by name (tieBreakOrder), one class is an identity tie.
    fn tie_mixed_classes(a: Pos, b: Pos) -> i32 {
        let (class_a, class_b) = (a.z() & 1, b.z() & 1);
        if class_a == class_b {
            IDENTITY.with(|f| f.set(true));
            -1
        } else if class_a < class_b {
            -1
        } else {
            1
        }
    }

    fn fnv(text: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for byte in text.bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn parse_pos(text: &str) -> Pos {
        let mut parts = text.split(',').map(|p| p.parse::<i32>().unwrap());
        Pos::new(parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap())
    }

    // Each line of jmap_order.txt is a scenario of puts (+), removes (-) and
    // replaces (=) that JMapOrder.java ran on a java.util.HashMap<Pos, Integer>
    // (the value is the op's index) and, puts only, on a java.util.HashSet<Pos>,
    // with the tree bins of JDK 25's HashMap, and the order each gave: its
    // size and the FNV-1a hash of its entries. A scenario in which the JVM
    // ordered two keys by System.identityHashCode (flag 1) is not
    // reproducible: the port must refuse there, and exactly there. JMAP_FIXTURE
    // names another file of the same form (make.sh makes one).
    #[test]
    fn hash_map_and_set_iterate_as_java() {
        let text = match std::env::var("JMAP_FIXTURE") {
            Ok(path) => std::fs::read_to_string(path).unwrap(),
            Err(_) => include_str!("jmap_fixture/jmap_order.txt").to_string(),
        };
        let (mut compared, mut refused) = (0, 0);
        for (scenario, line) in text.lines().enumerate() {
            let mut parts = line.split(" | ");
            let ops = parts.next().unwrap();
            let mut meta = parts.next().unwrap().split(' ');
            let tie = meta.next().unwrap();
            assert_ne!(tie, "?", "scenario {scenario}: the fixture needs the patched HashMap's tie flag");
            let (map_size, map_hash, set_size, set_hash) = (
                meta.next().unwrap().parse::<usize>().unwrap(),
                u64::from_str_radix(meta.next().unwrap(), 16).unwrap(),
                meta.next().unwrap().parse::<usize>().unwrap(),
                u64::from_str_radix(meta.next().unwrap(), 16).unwrap(),
            );
            let mut ops = ops.split(' ');
            let tie_break = if ops.next().unwrap() == "M" { tie_mixed_classes } else { tie_same_class };
            IDENTITY.with(|f| f.set(false));
            let mut map: JavaHashMap<i32> = JavaHashMap::with_tie_break(tie_break);
            let mut set: JavaHashMap<()> = JavaHashMap::with_tie_break(tie_break);
            for (i, op) in ops.enumerate() {
                let pos = parse_pos(&op[1..]);
                match op.as_bytes()[0] {
                    b'+' => {
                        map.put(pos, i as i32);
                        set.put(pos, ());
                    }
                    b'-' => map.remove(pos),
                    b'=' => map.replace(pos, i as i32),
                    other => panic!("op {}", other as char),
                }
                if IDENTITY.with(|f| f.get()) {
                    break;
                }
            }
            let ties = IDENTITY.with(|f| f.get());
            assert_eq!(ties, tie == "1", "scenario {scenario}: refusal on an identity tie");
            if ties {
                refused += 1;
                continue;
            }
            let entries = map.entries();
            let got: String = entries.iter().map(|(at, v)| format!("{},{},{}:{};", at.x(), at.y(), at.z(), v)).collect();
            assert_eq!((entries.len(), fnv(&got)), (map_size, map_hash), "scenario {scenario}: HashMap order");
            let keys = set.entries();
            let got: String = keys.iter().map(|(at, _)| format!("{},{},{};", at.x(), at.y(), at.z())).collect();
            assert_eq!((keys.len(), fnv(&got)), (set_size, set_hash), "scenario {scenario}: HashSet order");
            compared += 1;
        }
        eprintln!("jmap: {compared} scenarios identical, {refused} refused on identity ties");
        assert!(compared > 0 && refused > 0);
    }

    // The engine's map refuses, not guesses, on a tie: nine keys of one hash
    // bucket in a table of 64 treeify, and two with one full hash are ordered
    // by identityHashCode.
    #[test]
    fn equal_hash_keys_in_a_tree_refuse() {
        let result = std::panic::catch_unwind(|| {
            let mut set = JavaPosSet::new();
            // A table of 64 first, then ten keys of one low-6-bit bucket,
            // two of them (x, y, z) and (x, y - 31, z + 1) of one hash.
            for i in 0..30 {
                set.insert(Pos::new(i, 0, 0));
            }
            for i in 0..8 {
                set.insert(Pos::new(64 * i + 5, 100, 0));
            }
            set.insert(Pos::new(5, 100 - 31, 1));
        });
        let payload = result.unwrap_err();
        assert!(payload.downcast_ref::<crate::level::Refusal>().is_some());
    }
}
