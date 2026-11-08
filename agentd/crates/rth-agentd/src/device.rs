

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Device {
    pub kind: &'static str,
    pub kind_pt: &'static str,
}

pub const MAC: Device = Device { kind: "Mac", kind_pt: "Mac" };
pub const MACBOOK: Device = Device { kind: "MacBook", kind_pt: "MacBook" };
pub const MAC_MINI: Device = Device { kind: "Mac mini", kind_pt: "Mac mini" };
pub const IMAC: Device = Device { kind: "iMac", kind_pt: "iMac" };
pub const MAC_PRO: Device = Device { kind: "Mac Pro", kind_pt: "Mac Pro" };
pub const LINUX: Device = Device { kind: "Linux computer", kind_pt: "computador Linux" };
pub const WINDOWS: Device = Device { kind: "Windows PC", kind_pt: "PC Windows" };
pub const COMPUTER: Device = Device { kind: "computer", kind_pt: "computador" };

pub const ALL: [Device; 8] = [MAC, MACBOOK, MAC_MINI, IMAC, MAC_PRO, LINUX, WINDOWS, COMPUTER];

pub fn this() -> &'static Device {
    static DEVICE: OnceLock<Device> = OnceLock::new();
    DEVICE.get_or_init(|| from_facts(std::env::consts::OS, hw_model().as_deref()))
}

pub fn from_facts(os: &str, hw_model: Option<&str>) -> Device {
    match os {
        "macos" => match hw_model.unwrap_or_default() {
            m if m.starts_with("MacBook") => MACBOOK,
            m if m.starts_with("Macmini") => MAC_MINI,
            m if m.starts_with("iMac") => IMAC,
            m if m.starts_with("MacPro") => MAC_PRO,
            _ => MAC,
        },
        "linux" => LINUX,
        "windows" => WINDOWS,
        _ => COMPUTER,
    }
}

fn hw_model() -> Option<String> {
    if std::env::consts::OS != "macos" {
        return None;
    }
    let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "hw.model"]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_kind_comes_from_the_os_and_model_code_and_never_from_a_name() {
        assert_eq!(from_facts("macos", Some("MacBookPro18,3")), MACBOOK);
        assert_eq!(from_facts("macos", Some("MacBookAir10,1")), MACBOOK);
        assert_eq!(from_facts("macos", Some("Macmini9,1")), MAC_MINI);
        assert_eq!(from_facts("macos", Some("iMac21,1")), IMAC);
        assert_eq!(from_facts("macos", Some("MacPro7,1")), MAC_PRO);
        assert_eq!(from_facts("macos", Some("Mac14,2")), MAC, "Apple silicon codes name no kind");
        assert_eq!(from_facts("macos", None), MAC);
        assert_eq!(from_facts("linux", None), LINUX);
        assert_eq!(from_facts("windows", None), WINDOWS);
        assert_eq!(from_facts("freebsd", None), COMPUTER);

        assert!([MAC, MACBOOK, MAC_MINI, IMAC, MAC_PRO, LINUX, WINDOWS, COMPUTER].contains(this()));
    }
}
