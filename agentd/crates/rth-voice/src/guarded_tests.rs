
use super::*;

#[test]
fn veto_is_silent_and_changes_no_queue_or_echo_evidence() {
    let q = PlayQueue::new(48_000, 100);
    q.push(7, vec![11; 4]);
    let mut out = [99; 8];
    let mut calls = 0;
    assert_eq!(q.pull_guarded(&mut out, |e, cursor, n| {
        assert_eq!((e, cursor, n), (7, 0, 4));
        assert!(q.inner.try_lock().is_err(), "guard and copy share the queue lock");
        calls += 1;
        false
    }), 0);
    assert_eq!(calls, 1);
    assert_eq!(out, [0; 8], "no stale callback-buffer data");
    assert_eq!(q.queued_for(7), 4, "veto never consumes queued PCM");
    assert_eq!(q.rendered(7).samples, 0);
    assert!(q.rendered(7).first_at.is_none() && q.rendered(7).last_at.is_none());
    let s = q.stats();
    assert_eq!((s.pulls, s.underrun_samples, s.rendered_after_fence), (0, 0, 0));
    assert!(q.drain_tap().is_empty(), "veto feeds no H3 reference");
    assert_eq!(q.pull_guarded(&mut out, |_, _, _| true), 4);
    assert_eq!(out, [11, 11, 11, 11, 0, 0, 0, 0]);
}

#[test]
fn accepted_prefix_survives_a_later_veto_without_counting_the_tail() {
    let q = PlayQueue::new(48_000, 100);
    q.push(2, vec![20; 3]); q.push(3, vec![30; 4]);
    let mut out = [99; 8];
    let mut seen = Vec::new();
    assert_eq!(q.pull_guarded(&mut out, |e, cursor, n| {
        seen.push((e, cursor, n)); e == 2
    }), 3);
    assert_eq!(seen, [(2, 0, 3), (3, 0, 4)]);
    assert_eq!(out, [20, 20, 20, 0, 0, 0, 0, 0]);
    assert_eq!((q.rendered(2).samples, q.rendered(3).samples, q.queued_for(3)), (3, 0, 4));
    assert_eq!(q.stats().underrun_samples, 0, "policy veto is not starvation");
    assert_eq!(q.stats().pulls, 1, "the admitted prefix counts as one pull");
    let tap = q.drain_tap();
    assert_eq!(tap.len(), 1);
    assert_eq!((tap[0].epoch, tap[0].epoch_sample, tap[0].samples, tap[0].real), (Some(2), 0, 3, 3));
    assert!((tap[0].sum_sq - 3.0 * (20.0f64 / 32768.0).powi(2)).abs() < 1e-15);
}

#[test]
fn guard_receives_exact_epoch_cursor_and_size_before_each_segment() {
    let q = PlayQueue::new(48_000, 100);
    q.push(4, vec![1; 3]); q.push(4, vec![2; 2]); q.push(5, vec![3; 3]);
    let mut first = [0; 2];
    assert_eq!(q.pull_guarded(&mut first, |e, cursor, n| { assert_eq!((e, cursor, n), (4, 0, 2)); true }), 2);
    let mut seen = Vec::new(); let mut out = [0; 5];
    assert_eq!(q.pull_guarded(&mut out, |e, cursor, n| { seen.push((e, cursor, n)); true }), 5);
    assert_eq!(seen, [(4, 2, 1), (4, 3, 2), (5, 0, 2)]);
    assert_eq!(out, [1, 2, 2, 3, 3]);
    let taps = q.drain_tap();
    assert_eq!(taps.iter().map(|t| (t.epoch, t.epoch_sample, t.real)).collect::<Vec<_>>(),
        [(Some(4), 0, 2), (Some(4), 2, 1), (Some(4), 3, 2), (Some(5), 0, 2)]);
    assert_eq!((q.rendered(4).samples, q.rendered(5).samples, q.queued_for(5)), (5, 2, 1));
    for (tap, (count, sample)) in taps.iter().zip([(2, 1), (1, 1), (2, 2), (2, 3)]) {
        let energy = count as f64 * (sample as f64 / 32768.0).powi(2);
        assert!((tap.sum_sq - energy).abs() < 1e-15, "each tap covers only its own segment");
    }
}

#[test]
fn prior_flush_and_empty_output_never_call_guard_or_reanimate_samples() {
    let q = PlayQueue::new(48_000, 100);
    q.push(1, vec![9; 10]); q.flush_through(1);
    let mut out = [99; 4];
    assert_eq!(q.pull_guarded(&mut out, |_, _, _| panic!("flushed segment")), 0);
    assert_eq!(out, [0; 4]);
    assert_eq!(q.rendered(1).samples, 0);
    let taps = q.drain_tap(); assert_eq!(taps.len(), 1, "empty queue emits one silence reference");
    assert_eq!((taps[0].samples, taps[0].real, taps[0].epoch, taps[0].epoch_sample, taps[0].sum_sq), (4, 0, None, 0, 0.0));
    q.push(2, vec![8; 3]);
    let pulls = q.stats().pulls;
    assert_eq!(q.pull_guarded(&mut [], |_, _, _| panic!("empty output")), 0);
    assert_eq!(q.stats().pulls, pulls);
    assert_eq!(q.queued_for(2), 3);
}

#[test]
fn atomic_cancel_fence_and_render_count_agree() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let q = PlayQueue::new(48_000, 100);
    let state = AtomicU64::new(0); const STOP: u64 = 1 << 63;
    let admit = |_: u64, _: u64, n: usize| state.fetch_update(Ordering::SeqCst, Ordering::SeqCst,
        |s| (s & STOP == 0).then_some(s + n as u64)).is_ok();
    q.push(6, vec![1; 8]); let mut out = [99; 4];
    assert_eq!(q.pull_guarded(&mut out, admit), 4);
    assert_eq!(state.fetch_or(STOP, Ordering::SeqCst), 4);
    assert_eq!(q.pull_guarded(&mut out, admit), 0);
    assert_eq!(out, [0; 4]);
    assert_eq!(q.rendered(6).samples, 4);
    assert_eq!(state.load(Ordering::SeqCst) & !STOP, 4);
    assert_eq!(q.drain_tap().len(), 1);
}

#[test]
fn allowed_single_segment_keeps_legacy_pull_accounting() {
    let a = PlayQueue::new(48_000, 100); let b = PlayQueue::new(48_000, 100);
    a.push(1, vec![4; 3]); b.push(1, vec![4; 3]);
    let mut x = [99; 5]; let mut y = [99; 5];
    assert_eq!(a.pull(&mut x), b.pull_guarded(&mut y, |_, _, _| true));
    assert_eq!(x, y);
    let (sa, sb) = (a.stats(), b.stats());
    assert_eq!((sa.queued, sa.pulls, sa.underrun_samples), (sb.queued, sb.pulls, sb.underrun_samples));
    assert_eq!(a.rendered(1).samples, b.rendered(1).samples);
    let (ta, tb) = (a.drain_tap(), b.drain_tap());
    assert_eq!(b.rendered(1).first_at, Some(tb[0].at), "accepted audio drives Playback Started");
    assert_eq!(b.rendered(1).last_at, Some(tb[0].at));
    assert_eq!((ta[0].samples, ta[0].real, ta[0].sum_sq), (tb[0].samples, tb[0].real, tb[0].sum_sq));
}

#[test]
fn tap_ring_stays_bounded() {
    let q = PlayQueue::new(48_000, 2); let mut out = [0; 1];
    for _ in 0..TAP_CAP + 2 {
        q.push(1, vec![1]); assert_eq!(q.pull_guarded(&mut out, |_, _, _| true), 1);
    }
    let taps = q.drain_tap(); assert_eq!(taps.len(), TAP_CAP);
    assert_eq!(taps[0].epoch_sample, 2);
}

#[test]
#[ignore = "CPU micro-measurement; run explicitly with the CPU test gate"]
fn per_segment_cost() {
    use std::{hint::black_box, sync::atomic::{AtomicU64, Ordering}, time::Instant};
    const COUNT: usize = 1024;
    for frames in [1usize, 48, 480] {
        let mut baseline = Vec::new(); let mut guarded = Vec::new();
        let mut out = vec![0; frames];
        for batch in 0..32 {
            for use_guard in [false, true] {
                let q = PlayQueue::new(48_000, frames * COUNT);
                for _ in 0..COUNT { q.push(1, vec![1; frames]); }

                for _ in 0..COUNT { q.pull(&mut out); }
                for _ in 0..COUNT { q.push(1, vec![1; frames]); }
                let counter = AtomicU64::new(0);
                let start = Instant::now();
                for _ in 0..COUNT {
                    if use_guard {
                        black_box(q.pull_guarded(black_box(&mut out), |_, _, n| {
                            counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |s| Some(s + n as u64)).is_ok()
                        }));
                    } else { black_box(q.pull(black_box(&mut out))); }
                }
                let ns = start.elapsed().as_nanos() as f64 / COUNT as f64;
                if use_guard { assert_eq!(counter.load(Ordering::SeqCst), (frames * COUNT) as u64); }
                if batch > 0 { if use_guard { guarded.push(ns); } else { baseline.push(ns); } }
            }
        }
        baseline.sort_by(f64::total_cmp); guarded.sort_by(f64::total_cmp);
        eprintln!("guard_cost frames={frames} segments_per_batch={COUNT} batches=31 pull_p50_ns={:.1} guarded_p50_ns={:.1} guarded_p90_ns={:.1} guarded_max_ns={:.1} delta_p50_ns={:.1}",
            baseline[15], guarded[15], guarded[27], guarded[30], guarded[15]-baseline[15]);
    }
}
