

use std::any::Any;
use std::ffi::c_void;
use std::sync::{Arc, Mutex, Weak};
use rth_voice::queue::PlayQueue;

pub const RATE: u32 = 48_000;

pub const HEADPHONES: bool = false;
const MAX_FRAMES: usize = 8192;
const GLOBAL: u32 = 0;
const INPUT: u32 = 1;
const OUTPUT: u32 = 2;
type Unit = *mut c_void;
type Status = i32;
#[repr(C)]
struct Description { kind: u32, subtype: u32, manufacturer: u32, flags: u32, mask: u32 }
#[repr(C)]
struct Format { rate: f64, id: u32, flags: u32, bytes_packet: u32, frames_packet: u32, bytes_frame: u32, channels: u32, bits: u32, reserved: u32 }
#[repr(C)]
struct Buffer { channels: u32, bytes: u32, data: *mut c_void }
#[repr(C)]
struct Buffers { count: u32, buffer: Buffer }
type CallbackFn = unsafe extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut Buffers) -> Status;
#[repr(C)]
struct Callback { call: CallbackFn, context: *mut c_void }
#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioComponentFindNext(previous: Unit, description: *const Description) -> Unit;
    fn AudioComponentInstanceNew(component: Unit, instance: *mut Unit) -> Status;
    fn AudioComponentInstanceDispose(unit: Unit) -> Status;
    fn AudioUnitSetProperty(unit: Unit, property: u32, scope: u32, element: u32, value: *const c_void, size: u32) -> Status;
    fn AudioUnitInitialize(unit: Unit) -> Status;
    fn AudioUnitUninitialize(unit: Unit) -> Status;
    fn AudioOutputUnitStart(unit: Unit) -> Status;
    fn AudioOutputUnitStop(unit: Unit) -> Status;
    fn AudioUnitRender(unit: Unit, flags: *mut u32, time: *const c_void, bus: u32, frames: u32, buffers: *mut Buffers) -> Status;
}
type Sink = Box<dyn FnMut(Vec<f32>) + Send>;
struct Capture { unit: Unit, sink: Sink, samples: Vec<f32>, probe: crate::doubletalk::Probe, front: Option<crate::voice_front::capture::Reader> }

unsafe impl Send for Capture {}
struct Attachment {queue:Arc<PlayQueue>,binding:Option<Arc<crate::voice_front::render::Binding>>}
struct Render { queue: Mutex<Option<Attachment>>, samples: Mutex<Vec<i16>> }
struct Engine { unit: Unit, _capture: Box<Capture>, render: Arc<Render> }
unsafe impl Send for Engine {}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            AudioOutputUnitStop(self.unit);
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
        }
    }
}
static RENDER: Mutex<Weak<Render>> = Mutex::new(Weak::new());
fn checked(status: Status, operation: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("VoiceProcessingIO {operation}: OSStatus {status}")) }
}
unsafe fn set<T>(unit: Unit, property: u32, scope: u32, bus: u32, value: &T) -> Result<(), String> {
    checked(AudioUnitSetProperty(unit, property, scope, bus, value as *const T as *const c_void, std::mem::size_of::<T>() as u32), &format!("property {property}/{scope}/{bus}"))
}
unsafe extern "C" fn capture(context: *mut c_void, flags: *mut u32, time: *const c_void, _: u32, frames: u32, _: *mut Buffers) -> Status {
    let c = &mut *(context as *mut Capture);
    let n = frames as usize;
    if n > c.samples.len() { return -50; }
    let mut buffers = Buffers { count: 1, buffer: Buffer { channels: 1, bytes: frames * 4, data: c.samples.as_mut_ptr().cast() } };
    let status = AudioUnitRender(c.unit, flags, time, 1, frames, &mut buffers);
    if status == 0 { c.probe.push_front(&c.samples[..n], c.front.as_ref()); (c.sink)(c.samples[..n].to_vec()); }
    status
}
unsafe extern "C" fn render(context: *mut c_void, _: *mut u32, _: *const c_void, _: u32, frames: u32, buffers: *mut Buffers) -> Status {
    if buffers.is_null() { return -50; }
    let r = &*(context as *const Render);

    let b = &mut *buffers;
    if b.count != 1 || b.buffer.data.is_null() || b.buffer.bytes < frames * 4 { return -50; }
    let out = std::slice::from_raw_parts_mut(b.buffer.data.cast::<f32>(), frames as usize);
    out.fill(0.0);
    if crate::doubletalk::playout_held() { crate::bc_playout::rendered(frames as usize,0,crate::doubletalk::reply_active());return 0; }
    let mut real=0;
    if let (Ok(queue), Ok(mut samples)) = (r.queue.try_lock(), r.samples.try_lock()) {
        if let Some(q) = queue.as_ref() {
            if out.len() <= samples.len() {
                real=match &q.binding {
                    Some(b)=>b.pull(&q.queue,&mut samples[..out.len()],crate::doubletalk::playout_held()),
                    None=>crate::speaker::pull_playout(&q.queue,&mut samples[..out.len()],crate::doubletalk::playout_held())};
                for (o, s) in out.iter_mut().zip(samples.iter()) { *o = *s as f32 / 32768.0; }
                crate::doubletalk::playing(samples[..out.len()].iter().any(|s|*s!=0));
            }
        }
    }
    crate::bc_playout::rendered(frames as usize,real,crate::doubletalk::reply_active());
    0
}

#[allow(dead_code)]
pub fn open(sink: Sink) -> Result<(Box<dyn Any + Send>, u32, String), String> { open_front(sink, None) }
pub fn open_front(sink: Sink, front: Option<crate::voice_front::capture::Reader>) -> Result<(Box<dyn Any + Send>, u32, String), String> {
    crate::doubletalk::reset();
    unsafe {
        let description = Description { kind: u32::from_be_bytes(*b"auou"), subtype: u32::from_be_bytes(*b"vpio"), manufacturer: u32::from_be_bytes(*b"appl"), flags: 0, mask: 0 };
        let component = AudioComponentFindNext(std::ptr::null_mut(), &description);
        if component.is_null() { return Err("macOS VoiceProcessingIO unavailable; use headphones fallback".into()); }
        let mut unit = std::ptr::null_mut();
        checked(AudioComponentInstanceNew(component, &mut unit), "create")?;
        let mut engine = Box::new(Engine {
            unit,
            _capture: Box::new(Capture { unit, sink, samples: vec![0.0; MAX_FRAMES], probe: crate::doubletalk::Probe::default(), front }),
            render: Arc::new(Render { queue: Mutex::new(None), samples: Mutex::new(vec![0; MAX_FRAMES]) }),
        });

        set(unit, 2003, INPUT, 1, &1u32)?;
        set(unit, 2003, OUTPUT, 0, &1u32)?;
        set(unit, 14, GLOBAL, 0, &(MAX_FRAMES as u32))?;
        let format = Format { rate: RATE as f64, id: u32::from_be_bytes(*b"lpcm"), flags: 1 | 8, bytes_packet: 4, frames_packet: 1, bytes_frame: 4, channels: 1, bits: 32, reserved: 0 };
        set(unit, 8, OUTPUT, 1, &format)?;
        set(unit, 8, INPUT, 0, &format)?;
        set(unit, 2100, GLOBAL, 0, &u32::from(HEADPHONES))?;
        set(unit, 2101, GLOBAL, 0, &0u32)?;
        set(unit, 2005, GLOBAL, 1, &Callback { call: capture, context: (&mut *engine._capture as *mut Capture).cast() })?;
        set(unit, 23, INPUT, 0, &Callback { call: render, context: Arc::as_ptr(&engine.render) as *mut c_void })?;
        checked(AudioUnitInitialize(unit), "initialize")?;
        checked(AudioOutputUnitStart(unit), "start")?;
        if let Some(front)=engine._capture.front.as_ref() {front.activate();}
        *RENDER.lock().unwrap() = Arc::downgrade(&engine.render);
        Ok((engine, RATE, if HEADPHONES { "VoiceProcessingIO (headphones, AEC bypassed)" } else { "VoiceProcessingIO (speakers, AEC enabled)" }.into()))
    }
}
struct Playback { render: Arc<Render>, queue: Arc<PlayQueue> }
impl Drop for Playback {
    fn drop(&mut self) {
        let mut q = self.render.queue.lock().unwrap();
        if q.as_ref().is_some_and(|q| Arc::ptr_eq(&q.queue, &self.queue)) { *q = None; crate::doubletalk::playing(false); }
    }
}
pub fn output(queue: Arc<PlayQueue>) -> Result<Box<dyn Any + Send>, String> {
    attach(queue,None)
}
pub fn output_front(queue:Arc<PlayQueue>,binding:Arc<crate::voice_front::render::Binding>) -> Result<Box<dyn Any+Send>,String> {
    attach(queue,Some(binding))
}
fn attach(queue:Arc<PlayQueue>,binding:Option<Arc<crate::voice_front::render::Binding>>) -> Result<Box<dyn Any+Send>,String> {
    let render = RENDER.lock().unwrap().upgrade().ok_or("Conversation audio is not ready")?;
    *render.queue.lock().unwrap() = Some(Attachment {queue:queue.clone(),binding});
    crate::doubletalk::playing(false);
    Ok(Box::new(Playback { render, queue }))
}

#[cfg(test)]
mod attachment_tests {
    use super::*;
    #[test] fn dropping_old_playback_cannot_detach_the_new_queue_or_capture() {
        let render=Arc::new(Render {queue:Mutex::new(None),samples:Mutex::new(vec![0;16])});
        let old=Arc::new(PlayQueue::new(RATE,16));let new=Arc::new(PlayQueue::new(RATE,16));
        let first=Playback {render:render.clone(),queue:old};
        *render.queue.lock().unwrap()=Some(Attachment {queue:new.clone(),binding:None});
        drop(first);assert!(Arc::ptr_eq(&render.queue.lock().unwrap().as_ref().unwrap().queue,&new));
        drop(Playback {render:render.clone(),queue:new});assert!(render.queue.lock().unwrap().is_none());
        assert_eq!(render.samples.lock().unwrap().len(),16);
    }
}

#[cfg(test)]
mod front_callback_tests {
    use super::*;
    #[test]
    fn fake_vpio_render_uses_guarded_front_attachment_and_its_actual_first_at() {
        use crate::voice_front::{runtime::{Epoch,Ticket},render::Binding};

        const MARK:&str="RTH_VPIO_FRONT_CALLBACK_FIXTURE";
        if std::env::var_os(MARK).is_none() {
            let mut child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","duplex_audio::front_callback_tests::fake_vpio_render_uses_guarded_front_attachment_and_its_actual_first_at","--test-threads","4"])
                .env(MARK,"1").spawn().unwrap();
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(20);
            let status=loop {
                if let Some(status)=child.try_wait().unwrap(){break status;}
                if std::time::Instant::now()>=deadline {let _=child.kill();let _=child.wait();panic!("fake VPIO child deadline");}
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert!(status.success());return;
        }
        crate::doubletalk::reset();let e=Epoch{capture:10,turn:20};let t=Ticket::new(e,"vpio-front".into());
        let b=Arc::new(Binding::new(t.clone(),e));assert!(b.bind(1));let q=Arc::new(PlayQueue::new(RATE,16));q.push(1,vec![100,200,300,400]);
        let r=Render {queue:Mutex::new(Some(Attachment{queue:q.clone(),binding:Some(b.clone())})),samples:Mutex::new(vec![0;16])};
        let mut out=[-1.0_f32;2];let mut buffers=Buffers {count:1,buffer:Buffer{channels:1,bytes:8,data:out.as_mut_ptr().cast()}};
        let pull=|buffers:&mut Buffers|unsafe {render((&r as *const Render).cast_mut().cast(),std::ptr::null_mut(),std::ptr::null(),0,2,buffers)};
        assert_eq!(pull(&mut buffers),0);assert_eq!(t.frames(),2);assert_eq!(out,[100.0/32768.0,200.0/32768.0]);
        let first=q.rendered(1).first_at.unwrap();assert!(b.times().1.is_some());assert_eq!(q.drain_tap()[0].at,first);
        t.close();out.fill(-1.0);assert_eq!(pull(&mut buffers),0);assert_eq!(out,[0.0;2]);
        assert_eq!(q.rendered(1).samples,2);assert_eq!(t.frames(),2);assert!(q.drain_tap().is_empty());assert!(b.flush_veto(&q));assert_eq!(q.stats().queued,0);
    }
}
