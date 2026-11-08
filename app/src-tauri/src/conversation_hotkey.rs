

use std::ffi::c_void;
use std::sync::{atomic::{AtomicBool, Ordering}, OnceLock};

pub const CONVERSATION_HOTKEY: &str = "Ctrl+Cmd+Shift+C";
const KEY_CODE: u32 = 8;
const MODIFIERS: u32 = (1 << 8) | (1 << 9) | (1 << 12);
const SIGNATURE: u32 = u32::from_be_bytes(*b"ufcv");

const KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const PRESSED: u32 = 5;
const RELEASED: u32 = 6;
static CALLBACK: OnceLock<fn()> = OnceLock::new();
static HELD: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Default)]
struct HotKeyId { signature: u32, id: u32 }
#[repr(C)]
struct EventType { class: u32, kind: u32 }
type Handler = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(target: *mut c_void, handler: Option<Handler>, count: u32, types: *const EventType, data: *mut c_void, installed: *mut *mut c_void) -> i32;
    fn RemoveEventHandler(handler: *mut c_void) -> i32;
    fn RegisterEventHotKey(code: u32, modifiers: u32, id: HotKeyId, target: *mut c_void, options: u32, hotkey: *mut *mut c_void) -> i32;
    fn GetEventParameter(event: *mut c_void, name: u32, kind: u32, actual_kind: *mut u32, size: u32, actual_size: *mut u32, value: *mut c_void) -> i32;
    fn GetEventKind(event: *mut c_void) -> u32;
}
fn pressed(held: &AtomicBool, kind: u32) -> bool {
    match kind {
        PRESSED => !held.swap(true, Ordering::SeqCst),
        RELEASED => { held.store(false, Ordering::SeqCst); false }
        _ => false,
    }
}
unsafe extern "C" fn handle(_: *mut c_void, event: *mut c_void, _: *mut c_void) -> i32 {
    let mut id = HotKeyId::default();
    let status = unsafe { GetEventParameter(event, u32::from_be_bytes(*b"----"), u32::from_be_bytes(*b"hkid"), std::ptr::null_mut(), std::mem::size_of::<HotKeyId>() as u32, std::ptr::null_mut(), (&mut id as *mut HotKeyId).cast()) };
    if status != 0 || id.signature != SIGNATURE || id.id != 1 { return -9874; }
    if pressed(&HELD, unsafe { GetEventKind(event) }) {
        if let Some(callback) = CALLBACK.get() { callback(); }
    }
    0
}

pub fn install(callback: fn()) -> Result<(), String> {
    if CALLBACK.set(callback).is_err() { return Ok(()); }
    let types = [EventType {class:KEYBOARD, kind:PRESSED}, EventType {class:KEYBOARD, kind:RELEASED}];
    let (mut handler, mut hotkey) = (std::ptr::null_mut(), std::ptr::null_mut());
    let target = unsafe { GetApplicationEventTarget() };
    let status = unsafe { InstallEventHandler(target, Some(handle), types.len() as u32, types.as_ptr(), std::ptr::null_mut(), &mut handler) };
    if status != 0 { return Err(format!("{CONVERSATION_HOTKEY}: event handler ({status})")); }
    let status = unsafe { RegisterEventHotKey(KEY_CODE, MODIFIERS, HotKeyId {signature:SIGNATURE,id:1}, target, 0, &mut hotkey) };
    if status != 0 { unsafe { RemoveEventHandler(handler); } return Err(format!("{CONVERSATION_HOTKEY}: unavailable ({status})")); }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_held_hotkey_toggles_once_then_rearms_on_release() {
        let held = AtomicBool::new(false);
        assert!(pressed(&held, PRESSED)); assert!(!pressed(&held, PRESSED));
        assert!(!pressed(&held, RELEASED)); assert!(pressed(&held, PRESSED));
        assert!(!pressed(&held, 0));
    }
    #[test]
    fn review_r3_carbon_hotkey_event_values_rearm_the_chord_on_every_release() {

        assert_eq!((PRESSED, RELEASED), (5, 6));
        assert_eq!(KEYBOARD, 0x6b657962);
        let held = AtomicBool::new(false);
        for _ in 0..3 {
            assert!(pressed(&held, 5));
            assert!(!pressed(&held, 5), "repeat stays suppressed");
            assert!(!pressed(&held, 6), "release only rearms");
        }
    }
    #[test]
    fn the_chord_is_the_named_control_command_shift_c() {
        assert_eq!((KEY_CODE, MODIFIERS), (8, 4864));
        assert_eq!(CONVERSATION_HOTKEY, "Ctrl+Cmd+Shift+C");
    }
}
