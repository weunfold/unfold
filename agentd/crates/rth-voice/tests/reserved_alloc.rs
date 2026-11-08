
use rth_voice::queue::{PlayQueue, Push, TAP_CAP};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
struct Count;
static ON: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ON.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if ON.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if ON.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Count = Count;
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    ALLOCS.store(0, Ordering::SeqCst);
    ON.store(true, Ordering::SeqCst);
    let result = f();
    ON.store(false, Ordering::SeqCst);
    (result, ALLOCS.load(Ordering::SeqCst))
}
#[test]
fn cold_reserved_front_reply_never_allocates_in_callback() {
    let mut total = 0;
    for reply in 1..=4 {
        let q = PlayQueue::new_reserved(48_000, 48_000 * 4);
        let mut out = [-1; 480];

        let (n, empty) =
            counted(|| q.pull_guarded(&mut out, |_, _, _| panic!("empty queue has no admission")));
        assert_eq!(
            empty, 0,
            "new_reserved must initialize its first queue lock before any callback"
        );
        assert_eq!(n, 0);
        assert!(out.iter().all(|s| *s == 0));

        let pristine = PlayQueue::new_reserved(48_000, 48_000 * 4);
        assert_eq!(pristine.rendered(reply).samples, 0);
        assert!(pristine.rendered(reply).first_at.is_none());
        assert_eq!(pristine.stats().pulls, 0);
        assert!(pristine.drain_tap().is_empty());

        let mut frames = 0;
        for i in 0..TAP_CAP * 2 + 17 {
            assert_eq!(q.push(reply, vec![123; 240]), Push::Queued);
            assert_eq!(q.push(reply, vec![456; 240]), Push::Queued);
            let (n, a) = counted(|| {
                q.pull_guarded(&mut out, |e, c, take| {
                    assert_eq!((e, c), (reply, frames));
                    frames += take as u64;
                    true
                })
            });
            assert_eq!(n, 480);
            total += a;

            if i == TAP_CAP + 16 || (i > TAP_CAP + 16 && i % 173 == 172) {
                let taps = q.drain_tap();
                if i == TAP_CAP + 16 {
                    assert_eq!(
                        taps.len(),
                        TAP_CAP,
                        "the tap ring must wrap before its first drain"
                    );
                } else {
                    assert!(!taps.is_empty());
                }
            }
        }
        assert_eq!(q.rendered(reply).samples, frames);
        assert!(q.rendered(reply).first_at.is_some());
        q.push(reply, vec![777; 480]);
        let before = q.stats();
        let (n, veto) = counted(|| q.pull_guarded(&mut out, |_, _, _| false));
        assert_eq!(n, 0);
        assert!(out.iter().all(|s| *s == 0));
        assert_eq!(q.stats().pulls, before.pulls);
        assert_eq!(q.queued_for(reply), 480);
        total += empty + veto;
    }
    println!(
        "reserved_front_callback_allocations replies=4 pulls_per_reply={} allocations={total}",
        TAP_CAP * 2 + 19
    );
    assert_eq!(
        total, 0,
        "fresh reply must reserve its render map and complete tap ring before callback"
    );
}
