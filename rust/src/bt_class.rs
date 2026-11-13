//! The console's Bluetooth device class, set through the kernel's management
//! interface (the socket bluetoothd itself uses).
//!
//! Knulli leaves the class at "Miscellaneous". Many car stereos accept a
//! pairing only from a device whose class says "phone" and refuse the others
//! at once ("Pairing Not Allowed", "Unacceptable BD_ADDR"); BlueZ's own
//! source notes a car kit that refused pairing without the telephony bit.
//! While the app runs, the console says it is a smartphone. Only the major
//! and minor class change: the service bits (audio, telephony) stay as the
//! kernel derives them from bluetoothd's profiles. bluetoothd writes its own
//! class again only when it starts, so the media worker checks every few
//! seconds and sets the phone class again when needed.
//!
//! `BETTERPOD_RUST_BT_PHONE=0` keeps Knulli's class.

/// Phone, in the five major-class bits.
const PHONE_MAJOR: u8 = 0x02;
/// Smartphone, in bits 2–7 of the class's first octet.
const SMARTPHONE_MINOR: u8 = 0x0c;

const READ_INFO: u16 = 0x0004;
const SET_DEVICE_CLASS: u16 = 0x000e;
const COMMAND_COMPLETE: u16 = 0x0001;
const COMMAND_STATUS: u16 = 0x0002;

/// Current-settings bits of Read Controller Information.
const POWERED: u32 = 1 << 0;
const BREDR: u32 = 1 << 7;

/// False when `BETTERPOD_RUST_BT_PHONE=0`.
pub fn enabled() -> bool {
    !std::env::var("BETTERPOD_RUST_BT_PHONE").is_ok_and(|v| v == "0")
}

/// What `ensure_phone` found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    /// Already a phone, or the controller is off (set once it is on).
    Unchanged,
    /// The class before and after.
    Changed { from: u32, to: u32 },
}

pub fn is_phone(class: u32) -> bool {
    (class >> 8) & 0x1f == u32::from(PHONE_MAJOR) && class & 0xfc == u32::from(SMARTPHONE_MINOR)
}

fn class_of(octets: &[u8]) -> Option<u32> {
    let c = octets.get(..3)?;
    Some(u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16)
}

/// Settings and class from a Read Controller Information reply: address 6,
/// version 1, manufacturer 2, supported settings 4, current settings 4,
/// class 3, then the names.
fn settings_and_class(info: &[u8]) -> Option<(u32, u32)> {
    let settings = u32::from_le_bytes(info.get(13..17)?.try_into().ok()?);
    Some((settings, class_of(info.get(17..20)?)?))
}

/// True when the class should become a phone's now. An unpowered controller
/// reports class 0 and keeps whatever is set; wait until it is on.
fn wants_phone(settings: u32, class: u32) -> bool {
    settings & POWERED != 0 && settings & BREDR != 0 && !is_phone(class)
}

fn command(opcode: u16, index: u16, params: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(6 + params.len());
    frame.extend_from_slice(&opcode.to_le_bytes());
    frame.extend_from_slice(&index.to_le_bytes());
    frame.extend_from_slice(&(params.len() as u16).to_le_bytes());
    frame.extend_from_slice(params);
    frame
}

/// The answer to `opcode` on controller `index`, if `event` is it: the
/// returned data, or the error status. Every other event is `None`; the
/// control socket also receives the events meant for bluetoothd.
fn reply_to(opcode: u16, index: u16, event: &[u8]) -> Option<Result<Vec<u8>, u8>> {
    let code = u16::from_le_bytes(event.get(0..2)?.try_into().ok()?);
    let from = u16::from_le_bytes(event.get(2..4)?.try_into().ok()?);
    let len = usize::from(u16::from_le_bytes(event.get(4..6)?.try_into().ok()?));
    let body = event.get(6..6 + len)?;
    if from != index || body.len() < 3 || u16::from_le_bytes([body[0], body[1]]) != opcode {
        return None;
    }
    match (code, body[2]) {
        (COMMAND_COMPLETE, 0) => Some(Ok(body[3..].to_vec())),
        (COMMAND_COMPLETE | COMMAND_STATUS, status) => Some(Err(status)),
        _ => None,
    }
}

fn status_name(status: u8) -> &'static str {
    match status {
        0x0a => "busy",
        0x0b => "rejected",
        0x0c => "not supported",
        0x0d => "invalid parameters",
        0x0f => "not powered",
        0x11 => "no such controller",
        0x12 => "blocked by rfkill",
        0x14 => "permission denied",
        _ => "failed",
    }
}

/// Makes controller `index` (0 for hci0) a smartphone, if it is not one.
pub fn ensure_phone(index: u16) -> Result<Outcome, String> {
    ensure_phone_with(|opcode, params| linux::request(opcode, index, params))
}

fn ensure_phone_with(
    mut request: impl FnMut(u16, &[u8]) -> Result<Vec<u8>, String>,
) -> Result<Outcome, String> {
    let info = request(READ_INFO, &[])?;
    let (settings, from) = settings_and_class(&info).ok_or("short controller information")?;
    if !wants_phone(settings, from) {
        return Ok(Outcome::Unchanged);
    }
    let reply = request(SET_DEVICE_CLASS, &[PHONE_MAJOR, SMARTPHONE_MINOR])?;
    let to = class_of(&reply).ok_or("short class reply")?;
    Ok(Outcome::Changed { from, to })
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{command, reply_to, status_name};
    use std::{
        io,
        os::fd::{AsRawFd, FromRawFd, OwnedFd},
        time::{Duration, Instant},
    };

    const AF_BLUETOOTH: libc::c_int = 31;
    const BTPROTO_HCI: libc::c_int = 1;
    const HCI_DEV_NONE: u16 = 0xffff;
    const HCI_CHANNEL_CONTROL: u16 = 3;

    #[repr(C)]
    struct SockaddrHci {
        family: libc::sa_family_t,
        device: u16,
        channel: u16,
    }

    fn open() -> io::Result<OwnedFd> {
        // SAFETY: plain socket(2); the descriptor is owned right after.
        let fd = unsafe {
            libc::socket(
                AF_BLUETOOTH,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                BTPROTO_HCI,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a fresh descriptor nobody else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let address = SockaddrHci {
            family: AF_BLUETOOTH as libc::sa_family_t,
            device: HCI_DEV_NONE,
            channel: HCI_CHANNEL_CONTROL,
        };
        // SAFETY: `address` is a valid sockaddr_hci of the given size.
        let bound = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                (&address as *const SockaddrHci).cast(),
                std::mem::size_of::<SockaddrHci>() as libc::socklen_t,
            )
        };
        if bound < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(fd)
    }

    /// The next event within `timeout`; `None` when nothing came.
    fn receive(fd: &OwnedFd, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ms = timeout.as_millis().clamp(1, 5_000) as libc::c_int;
        // SAFETY: one valid pollfd.
        if unsafe { libc::poll(&mut poll, 1, ms) } <= 0 {
            return Ok(None);
        }
        let mut buffer = vec![0u8; 4096];
        // SAFETY: reads at most the buffer's length into it.
        let read = unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if read < 0 {
            let error = io::Error::last_os_error();
            return match error.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => Ok(None),
                _ => Err(error),
            };
        }
        buffer.truncate(read as usize);
        Ok(Some(buffer))
    }

    pub fn request(opcode: u16, index: u16, params: &[u8]) -> Result<Vec<u8>, String> {
        let fd = open().map_err(|e| format!("management socket: {e}"))?;
        let frame = command(opcode, index, params);
        // SAFETY: writes the frame's bytes from a live buffer.
        let written = unsafe { libc::write(fd.as_raw_fd(), frame.as_ptr().cast(), frame.len()) };
        if written != frame.len() as isize {
            return Err(format!(
                "management command: {}",
                io::Error::last_os_error()
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("no answer from the Bluetooth controller".into());
            }
            let Some(event) = receive(&fd, left).map_err(|e| e.to_string())? else {
                continue;
            };
            match reply_to(opcode, index, &event) {
                Some(Ok(data)) => return Ok(data),
                Some(Err(status)) => {
                    return Err(format!("{} (status 0x{status:02x})", status_name(status)))
                }
                None => {}
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod linux {
    pub fn request(_opcode: u16, _index: u16, _params: &[u8]) -> Result<Vec<u8>, String> {
        Err("Bluetooth management needs Linux".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Read Controller Information reply with these settings and class.
    fn info(settings: u32, class: u32) -> Vec<u8> {
        let mut data = vec![0u8; 280];
        data[13..17].copy_from_slice(&settings.to_le_bytes());
        data[17..20].copy_from_slice(&class.to_le_bytes()[..3]);
        data
    }

    #[test]
    fn a_smartphone_class_is_a_phone_whatever_its_service_bits() {
        assert!(is_phone(0x5a020c));
        assert!(is_phone(0x00020c));
        assert!(!is_phone(0x6c0000), "Knulli's miscellaneous class");
        assert!(!is_phone(0x20041c), "portable audio");
        assert!(!is_phone(0x5a0204), "cellular phone, not smartphone");
    }

    #[test]
    fn frames_follow_the_management_layout() {
        assert_eq!(
            command(SET_DEVICE_CLASS, 0, &[PHONE_MAJOR, SMARTPHONE_MINOR]),
            vec![0x0e, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x0c]
        );
        let complete = [
            0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x0e, 0x00, 0x00, 0x0c, 0x02, 0x6c,
        ];
        assert_eq!(
            reply_to(SET_DEVICE_CLASS, 0, &complete),
            Some(Ok(vec![0x0c, 0x02, 0x6c]))
        );
        // bluetoothd's events and other controllers' answers are not ours
        assert_eq!(reply_to(READ_INFO, 0, &complete), None);
        assert_eq!(reply_to(SET_DEVICE_CLASS, 1, &complete), None);
        let found = [0x12, 0x00, 0x00, 0x00, 0x03, 0x00, 0x0e, 0x00, 0x00];
        assert_eq!(reply_to(SET_DEVICE_CLASS, 0, &found), None);
        let refused = [0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x0e, 0x00, 0x14];
        assert_eq!(reply_to(SET_DEVICE_CLASS, 0, &refused), Some(Err(0x14)));
        assert_eq!(reply_to(SET_DEVICE_CLASS, 0, &refused[..7]), None);
    }

    #[test]
    fn the_class_becomes_a_phone_once_and_only_while_powered() {
        let mut sent = Vec::new();
        let outcome = ensure_phone_with(|opcode, params| {
            sent.push((opcode, params.to_vec()));
            Ok(match opcode {
                READ_INFO => info(POWERED | BREDR, 0x6c0000),
                _ => vec![0x0c, 0x02, 0x6c],
            })
        });
        assert_eq!(
            outcome,
            Ok(Outcome::Changed {
                from: 0x6c0000,
                to: 0x6c020c
            })
        );
        assert_eq!(sent[1], (SET_DEVICE_CLASS, vec![0x02, 0x0c]));

        for (settings, class) in [(POWERED | BREDR, 0x6c020c), (BREDR, 0), (POWERED, 0x000000)] {
            let outcome = ensure_phone_with(|opcode, _| {
                assert_eq!(opcode, READ_INFO, "nothing to set");
                Ok(info(settings, class))
            });
            assert_eq!(outcome, Ok(Outcome::Unchanged));
        }
        assert!(ensure_phone_with(|_, _| Ok(vec![0; 10])).is_err());
    }
}
