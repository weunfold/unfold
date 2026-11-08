

use crate::dictation::{Mic, MicError, Permission};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::any::Any;
use std::ffi::c_void;

pub struct Cpal;

pub fn mono<T: Copy>(data: &[T], channels: usize, f: impl Fn(T) -> f32) -> Vec<f32> {
    if channels <= 1 {
        return data.iter().map(|&x| f(x)).collect();
    }
    data.chunks(channels).map(|fr| fr.iter().map(|&x| f(x)).sum::<f32>() / fr.len() as f32).collect()
}

impl Mic for Cpal {
    fn permission(&self) -> Permission {
        match unsafe { av::status() } {
            av::AUTHORIZED => Permission::Granted,
            av::NOT_DETERMINED => Permission::NotAsked,
            _ => Permission::Denied,
        }
    }

    fn ask(&self, answer: Box<dyn FnOnce(bool) + Send>) {
        unsafe { av::request(answer) }
    }

    fn open(&mut self, sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any>, u32, String), MicError> {
        open_default(sink).map(|(stream, rate, device)| (stream as Box<dyn Any>, rate, device))
    }

    fn opener(&self) -> Option<crate::dictation::Opener> {
        Some(Box::new(open_default))
    }
}

fn open_default(mut sink: Box<dyn FnMut(Vec<f32>) + Send>) -> Result<(Box<dyn Any + Send>, u32, String), MicError> {
    #[cfg(feature = "voice_pane")]
    if crate::conversation::active() { return crate::duplex_audio::open_front(sink, crate::front_native::capture_reader()).map_err(MicError::Other); }
    let host = cpal::default_host();
    let dev = host.default_input_device().ok_or_else(|| MicError::Other("no input device".into()))?;
    let cfg = dev.default_input_config().map_err(|e| MicError::Other(format!("input config: {e}")))?;
    let (rate, ch) = (cfg.sample_rate(), cfg.channels() as usize);
    let device = format!("{}, {ch} ch", dev.description().map_or_else(|e| format!("unnamed input ({e})"), |d| d.name().to_string()));
    let err = |e: cpal::Error| {

        let _ = std::io::Write::write_all(&mut std::io::stderr(), format!("rth-app: dictation input: {e}\n").as_bytes());
    };
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_input_stream(cfg.config(), move |d: &[f32], _: &_| sink(mono(d, ch, |x| x)), err, None),
        cpal::SampleFormat::I16 => dev.build_input_stream(cfg.config(), move |d: &[i16], _: &_| sink(mono(d, ch, |x| x as f32 / 32768.0)), err, None),
        cpal::SampleFormat::I32 => dev.build_input_stream(cfg.config(), move |d: &[i32], _: &_| sink(mono(d, ch, |x| x as f32 / 2147483648.0)), err, None),
        f => return Err(MicError::Other(format!("unsupported sample format {f:?}"))),
    }
    .map_err(|e| if format!("{e:?}").contains("PermissionDenied") { MicError::Denied } else { MicError::Other(format!("input stream: {e}")) })?;
    stream.play().map_err(|e| MicError::Other(format!("input play: {e}")))?;
    Ok((Box::new(stream), rate, device))
}

pub fn open_mic_settings() -> Result<(), String> {
    let ok = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?
        .success();
    if ok {
        Ok(())
    } else {
        Err("open failed".into())
    }
}

mod av {
    use super::c_void;
    use std::ffi::c_char;
    use std::sync::Mutex;

    pub const NOT_DETERMINED: isize = 0;
    pub const AUTHORIZED: isize = 3;

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: *mut c_void;
    }

    pub unsafe fn status() -> isize {
        let cls = objc_getClass(c"AVCaptureDevice".as_ptr());
        if cls.is_null() {
            return 2;
        }
        let send: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> isize = std::mem::transmute(objc_msgSend as *const c_void);
        send(cls, sel_registerName(c"authorizationStatusForMediaType:".as_ptr()), AVMediaTypeAudio)
    }

    pub unsafe fn request(answer: Box<dyn FnOnce(bool) + Send>) {
        let cls = objc_getClass(c"AVCaptureDevice".as_ptr());
        if cls.is_null() {
            answer(false);
            return;
        }
        let once = Mutex::new(Some(answer));
        let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
            if let Some(f) = once.lock().unwrap().take() {
                f(granted.as_bool());
            }
        });
        let send: extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) = std::mem::transmute(objc_msgSend as *const c_void);
        send(cls, sel_registerName(c"requestAccessForMediaType:completionHandler:".as_ptr()), AVMediaTypeAudio, block2::RcBlock::as_ptr(&block) as *mut c_void);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_frames_mix_to_mono() {
        assert_eq!(mono(&[1.0f32, 0.0, 0.5, 0.5], 2, |x| x), vec![0.5, 0.5]);
        assert_eq!(mono(&[16384i16, -16384], 1, |x| x as f32 / 32768.0), vec![0.5, -0.5]);
    }
}
