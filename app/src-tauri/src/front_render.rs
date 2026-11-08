
use super::runtime::{Epoch, Ticket};
use rth_voice::queue::PlayQueue;
use std::{sync::atomic::{AtomicBool,AtomicU64,Ordering},time::Instant};
pub struct Binding {
    pub ticket: Ticket, pub epoch: Epoch, queue_epoch: AtomicU64,
    veto: AtomicBool, veto_epoch: AtomicU64, started: Instant,
    first_pcm: AtomicU64, first_render: AtomicU64,
}
impl Binding {
    pub fn new(ticket: Ticket, epoch: Epoch) -> Self {
        Self {ticket,epoch,queue_epoch:AtomicU64::new(0),veto:AtomicBool::new(false),
            veto_epoch:AtomicU64::new(0),started:Instant::now(),first_pcm:AtomicU64::new(0),first_render:AtomicU64::new(0)}
    }

    pub fn bind(&self, epoch: u64) -> bool {
        epoch!=0 && self.queue_epoch.compare_exchange(0,epoch,Ordering::SeqCst,Ordering::SeqCst).is_ok()
    }
    fn now(&self) -> u64 {self.started.elapsed().as_nanos().min((u64::MAX-1) as u128) as u64+1}
    pub fn pcm(&self) {let _=self.first_pcm.compare_exchange(0,self.now(),Ordering::SeqCst,Ordering::SeqCst);}
    pub fn times(&self) -> (Option<u64>,Option<u64>) {
        let decode=|n:u64|n.checked_sub(1);
        (decode(self.first_pcm.load(Ordering::SeqCst)),decode(self.first_render.load(Ordering::SeqCst)))
    }

    pub fn pull(&self, q: &PlayQueue, out: &mut [i16], held: bool) -> usize {
        if held {out.fill(0);return 0;}
        let at=self.now();
        q.pull_guarded(out,|epoch,cursor,n|self.admit(epoch,cursor,n,at))
    }

    fn admit(&self,epoch:u64,cursor:u64,n:usize,at:u64) -> bool {
        let allowed=!self.veto.load(Ordering::SeqCst)
            && epoch==self.queue_epoch.load(Ordering::SeqCst) && epoch!=0
            && cursor==self.ticket.frames()
            && u32::try_from(n).ok().is_some_and(|n|self.ticket.render(self.epoch,self.ticket.response(),n));
        if allowed {let _=self.first_render.compare_exchange(0,at,Ordering::SeqCst,Ordering::SeqCst);}
        else {self.veto_epoch.fetch_max(epoch,Ordering::SeqCst);self.veto.store(true,Ordering::SeqCst);}
        allowed
    }

    pub fn reject(&self) {self.ticket.close();self.veto.store(true,Ordering::SeqCst);}

    pub fn flush_veto(&self, q: &PlayQueue) -> bool {
        if !self.veto.load(Ordering::SeqCst) {return false;}
        self.ticket.close();
        q.flush_through(self.veto_epoch.load(Ordering::SeqCst).max(self.queue_epoch.load(Ordering::SeqCst)));
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Binding, PlayQueue) {
        let epoch=Epoch {capture:7,turn:9};
        (Binding::new(Ticket::new(epoch,"response".into()),epoch),PlayQueue::new(48000,32))
    }
    #[test] fn accepted_render_counts_exact_frames_and_fence_veto_flushes_off_callback() {
        let (b,q)=fixture(); assert!(b.bind(3));q.push(3,vec![11,22,33,44]);
        let mut out=[-1;2];assert_eq!(b.pull(&q,&mut out,false),2);assert_eq!(out,[11,22]);
        assert_eq!(b.ticket.frames(),2);assert!(b.ticket.fence());
        let before=q.stats();let taps=q.drain_tap();assert_eq!(taps.len(),1);
        assert_eq!(b.pull(&q,&mut out,false),0);assert_eq!(out,[0,0]);
        assert_eq!(q.stats().queued,2);assert_eq!(q.stats().pulls,before.pulls);
        assert_eq!(q.rendered(3).samples,2);assert!(q.drain_tap().is_empty());
        assert!(b.flush_veto(&q));assert_eq!(q.stats().queued,0);assert_eq!(b.ticket.frames(),2);
    }
    #[test] fn wrong_epoch_cursor_and_unbound_output_fail_closed() {
        for mode in 0..3 {
            let (b,q)=fixture();if mode!=0 {assert!(b.bind(3));}
            q.push(if mode==1 {4}else{3},vec![8;4]);
            if mode==2 {assert!(b.ticket.render(b.epoch,b.ticket.response(),1));}
            let mut out=[9;4];assert_eq!(b.pull(&q,&mut out,false),0);assert_eq!(out,[0;4]);
            assert_eq!(q.rendered(3).samples,0);assert!(q.drain_tap().is_empty());
            assert!(b.flush_veto(&q));assert_eq!(q.stats().queued,0);
        }
    }
    #[test] fn held_output_never_admits_and_binding_cannot_be_replaced() {
        let (b,q)=fixture();assert!(b.bind(2));assert!(!b.bind(3));q.push(2,vec![3;4]);
        let mut out=[9;4];assert_eq!(b.pull(&q,&mut out,true),0);assert_eq!(out,[0;4]);
        assert_eq!(b.ticket.frames(),0);assert_eq!(q.stats().queued,4);assert!(q.drain_tap().is_empty());
        assert_eq!(b.pull(&q,&mut out,false),4);assert_eq!(b.ticket.frames(),4);
    }
    #[test] fn oversized_segment_and_foreign_capture_cannot_panic_or_claim_frames() {
        let (b,q)=fixture();b.bind(3);assert!(!b.admit(3,0,usize::MAX,1));
        assert_eq!(b.ticket.frames(),0);assert_eq!(b.times(),(None,None));assert!(b.flush_veto(&q));
        let (original,q)=fixture();let wrong=Binding::new(original.ticket.clone(),Epoch{capture:8,turn:9});
        wrong.bind(3);q.push(3,vec![2;2]);let mut out=[1;2];assert_eq!(wrong.pull(&q,&mut out,false),0);
        assert_eq!(out,[0;2]);assert_eq!(original.ticket.frames(),0);
    }
    #[test] fn multiple_segments_keep_exact_cursor_energy_and_first_timing() {
        let (b,q)=fixture();b.bind(3);b.pcm();let pcm=b.times().0.unwrap();
        q.push(3,vec![100;3]);q.push(3,vec![200;2]);let mut out=[0;5];assert_eq!(b.pull(&q,&mut out,false),5);
        assert_eq!(out,[100,100,100,200,200]);assert_eq!(b.ticket.frames(),5);
        let taps=q.drain_tap();assert_eq!(taps.len(),2);
        for (tap,(cursor,n,value)) in taps.iter().zip([(0,3,100.0_f64),(3,2,200.0_f64)]) {
            assert_eq!((tap.epoch,tap.epoch_sample,tap.real),(Some(3),cursor,n));
            assert!((tap.sum_sq-f64::from(n)*(value/32768.0).powi(2)).abs()<1e-15);
        }
        let first=b.times().1.unwrap();assert!(first>=pcm);
        q.push(3,vec![1]);b.pull(&q,&mut out[..1],false);assert_eq!(b.times(),(Some(pcm),Some(first)));
    }
    #[test] fn concurrent_cancel_has_exactly_one_winning_sample_boundary() {
        for _ in 0..64 {
            let (b,q)=fixture();b.bind(1);q.push(1,vec![7;8]);
            let b=std::sync::Arc::new(b);let gate=std::sync::Arc::new(std::sync::Barrier::new(2));
            let (other,g)=(b.clone(),gate.clone());let t=std::thread::spawn(move || {g.wait();other.ticket.fence()});
            gate.wait();let mut out=[9;8];let n=b.pull(&q,&mut out,false);let heard=t.join().unwrap();
            assert_eq!(n as u64,b.ticket.frames());assert_eq!(heard,n>0);
            assert_eq!(out,if n>0 {[7;8]}else{[0;8]});
            b.flush_veto(&q);
        }
    }
}

#[cfg(test)]
mod cost {
    use super::*;
    #[test]
    #[ignore = "explicit CPU callback cost receipt"]
    fn warmed_guarded_binding_cost_per_segment() {
        for segments in [1,8,32] {
            let epoch=Epoch{capture:1,turn:1};
            let b=Binding::new(Ticket::new(epoch,"cost".into()),epoch);b.bind(1);
            let q=PlayQueue::new(48000,segments*16);let mut out=vec![0;segments*16];let mut ns=Vec::new();
            for i in 0..2100 {
                for _ in 0..segments {q.push(1,vec![100;16]);}
                let at=Instant::now();let n=b.pull(&q,&mut out,false);let elapsed=at.elapsed().as_nanos();
                assert_eq!(n,out.len());if i>=100 {ns.push(elapsed);}
            }
            ns.sort_unstable();
            println!("binding_cost segments={segments} iterations={} median_ns={} p95_ns={} p99_ns={} max_ns={} median_ns_per_segment={}",ns.len(),ns[ns.len()/2],ns[ns.len()*95/100],ns[ns.len()*99/100],ns[ns.len()-1],ns[ns.len()/2]/segments as u128);
        }
    }
}
