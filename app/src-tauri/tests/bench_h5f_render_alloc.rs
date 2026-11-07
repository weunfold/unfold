
#[allow(dead_code)]
#[path = "../src/conversation_log.rs"]
mod conversation_log;
#[allow(dead_code)]
#[path = "../src/voice_front.rs"]
mod voice_front;
use rth_voice::queue::PlayQueue;
use voice_front::{
    render::Binding,
    runtime::{Epoch, Ticket},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering as CountOrder};
static COUNT_THREAD: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
struct Count;
fn note_allocation() {
    let owner = COUNT_THREAD.load(CountOrder::Relaxed);
    if owner != 0 && owner == unsafe { libc::pthread_self() } as usize {
        ALLOCS.fetch_add(1, CountOrder::Relaxed);
    }
}
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note_allocation();
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        note_allocation();
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        note_allocation();
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Count = Count;
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let id = unsafe { libc::pthread_self() } as usize;
    assert!(COUNT_THREAD
        .compare_exchange(0, id, CountOrder::SeqCst, CountOrder::SeqCst)
        .is_ok());
    ALLOCS.store(0, CountOrder::Relaxed);
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            COUNT_THREAD.store(0, CountOrder::SeqCst);
        }
    }
    let reset = Reset;
    let r = f();
    let n = ALLOCS.load(CountOrder::Relaxed);
    drop(reset);
    (r, n)
}

#[test]
fn six_successive_fresh_front_replies_have_zero_callback_allocations() {
    for turn in 1..=6 {
        let e = Epoch { capture: 1, turn };
        let b = Binding::new(Ticket::new(e, format!("reply-{turn}")), e);
        assert!(b.bind(1));
        let q = PlayQueue::new_reserved(48_000, 1 << 20);
        let mut out = [0; 480];
        let mut total = 0;
        assert_eq!(q.rendered(1).samples, 0);
        assert_eq!(q.stats().pulls, 0);
        for _ in 0..1100 {
            q.push(1, vec![1; 480]);
            let (n, a) = counted(|| b.pull(&q, &mut out, false));
            assert_eq!(n, 480);
            total += a;
        }
        q.push(1, vec![1; 240]);
        q.push(1, vec![1; 240]);
        let (n, a) = counted(|| b.pull(&q, &mut out, false));
        assert_eq!(n, 480);
        total += a;
        q.push(1, vec![1; 480]);
        let (_, a) = counted(|| b.pull(&q, &mut out, true));
        total += a;
        b.ticket.fence();
        for _ in 0..2 {
            let (n, a) = counted(|| b.pull(&q, &mut out, false));
            assert_eq!(n, 0);
            total += a;
        }
        println!("front_reply={turn} guarded_callback_allocations={total}");
        assert_eq!(total, 0);
    }
}
