//! BW's intrusive doubly-linked lists (node: +0 prev, +8 next), as index links.
//!
//! The exact insertion points matter: BW iterates these lists every frame, so
//! update/draw order (and therefore determinism) depends on them.

pub const NIL: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub prev: u32,
    pub next: u32,
}

impl Default for Link {
    fn default() -> Link {
        Link { prev: NIL, next: NIL }
    }
}

/// List head/tail pair (two globals in BW).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct List {
    pub head: u32,
    pub tail: u32,
}

impl Default for List {
    fn default() -> List {
        List { head: NIL, tail: NIL }
    }
}

pub trait Node {
    fn link(&self) -> &Link;
    fn link_mut(&mut self) -> &mut Link;
}

/// 0x1401956a0: insert `n` right after the head (or as the only element).
pub fn insert_after_head<T: Node>(v: &mut [T], n: u32, l: &mut List) {
    let h = l.head;
    if h == NIL {
        l.tail = n;
        l.head = n;
        return;
    }
    if l.tail == h {
        l.tail = n;
    }
    let hn = v[h as usize].link().next;
    *v[n as usize].link_mut() = Link { prev: h, next: hn };
    if hn != NIL {
        v[hn as usize].link_mut().prev = n;
    }
    v[h as usize].link_mut().next = n;
}

/// 0x1401956e0: insert `n` before `at`; `head` updated if `at` was the head.
pub fn insert_before<T: Node>(v: &mut [T], at: u32, n: u32, l: &mut List) {
    if l.head == at {
        l.head = n;
    }
    let ap = v[at as usize].link().prev;
    *v[n as usize].link_mut() = Link { prev: ap, next: at };
    if ap != NIL {
        v[ap as usize].link_mut().next = n;
    }
    v[at as usize].link_mut().prev = n;
}

/// 0x140195670: insert `n` after `at`; `tail` updated if `at` was the tail.
pub fn insert_after<T: Node>(v: &mut [T], at: u32, n: u32, l: &mut List) {
    if l.tail == at {
        l.tail = n;
    }
    let an = v[at as usize].link().next;
    *v[n as usize].link_mut() = Link { prev: at, next: an };
    if an != NIL {
        v[an as usize].link_mut().prev = n;
    }
    v[at as usize].link_mut().next = n;
}

/// 0x140195710: unlink `n`.
pub fn remove<T: Node>(v: &mut [T], n: u32, l: &mut List) {
    let Link { prev, next } = *v[n as usize].link();
    if l.head == n {
        l.head = next;
    }
    if l.tail == n {
        l.tail = prev;
    }
    if prev != NIL {
        v[prev as usize].link_mut().next = next;
    }
    if next != NIL {
        v[next as usize].link_mut().prev = prev;
    }
    *v[n as usize].link_mut() = Link::default();
}

/// Iterate from `start` following `next` links.
pub fn iter<T: Node>(v: &[T], start: u32) -> impl Iterator<Item = u32> + '_ {
    let mut cur = start;
    std::iter::from_fn(move || {
        if cur == NIL {
            return None;
        }
        let c = cur;
        cur = v[c as usize].link().next;
        Some(c)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default, Clone)]
    struct N(Link);
    impl Node for N {
        fn link(&self) -> &Link {
            &self.0
        }
        fn link_mut(&mut self) -> &mut Link {
            &mut self.0
        }
    }

    #[test]
    fn insert_after_head_order() {
        // BW quirk: each new node lands second, so 0,1,2,3 iterate as 0,3,2,1.
        let mut v = vec![N::default(); 4];
        let mut l = List::default();
        for i in 0..4 {
            insert_after_head(&mut v, i, &mut l);
        }
        assert_eq!(iter(&v, l.head).collect::<Vec<_>>(), vec![0, 3, 2, 1]);
        assert_eq!(l.tail, 1);
        remove(&mut v, 0, &mut l);
        assert_eq!(iter(&v, l.head).collect::<Vec<_>>(), vec![3, 2, 1]);
        insert_before(&mut v, 3, 0, &mut l);
        assert_eq!(iter(&v, l.head).collect::<Vec<_>>(), vec![0, 3, 2, 1]);
        remove(&mut v, 2, &mut l);
        insert_after(&mut v, 1, 2, &mut l);
        assert_eq!(iter(&v, l.head).collect::<Vec<_>>(), vec![0, 3, 1, 2]);
        assert_eq!(l.tail, 2);
    }
}
