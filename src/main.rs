use std::thread;
use std::fs::{self, OpenOptions};
use std::time::{Instant, Duration};
use std::arch::asm;
use std::sync::Mutex;
use std::ops::DerefMut;
use memmap::{MmapMut, MmapOptions};
use libc::iopl;
use once_cell::sync::Lazy;

#[derive(Copy, Clone, PartialEq)]
enum Joystick {
    Left = 1,
    Right = 2,
}

#[derive(Copy, Clone, PartialEq)]
enum LedPosition {
    Right = 1,
    Bottom = 2,
    Left = 3,
    Top = 4,
}

fn set_all_pixels(color: (u8, u8, u8)) {
    set_pixel(Joystick::Left, LedPosition::Right, color);
    set_pixel(Joystick::Left, LedPosition::Bottom, color);
    set_pixel(Joystick::Left, LedPosition::Left, color);
    set_pixel(Joystick::Left, LedPosition::Top, color);
    
    set_pixel(Joystick::Right, LedPosition::Right, color);
    set_pixel(Joystick::Right, LedPosition::Bottom, color);
    set_pixel(Joystick::Right, LedPosition::Left, color);
    set_pixel(Joystick::Right, LedPosition::Top, color);
}

fn set_pixel(js: Joystick, led: LedPosition, color: (u8, u8, u8)) {
    set_subpixel(js, led as u8 * 3, color.0);
    set_subpixel(js, led as u8 * 3 + 1, color.1);
    set_subpixel(js, led as u8 * 3 + 2, color.2);
}

fn set_subpixel(js: Joystick, subpixel_idx: u8, brightness: u8) {
    ec_cmd(js as u8, subpixel_idx, brightness);
    ec_cmd(0x3, 0x0, 0x0);
}

fn ec_cmd(cmd: u8, p1: u8, p2: u8) {
    ec_ram_write(0x6d, cmd);
    ec_ram_write(0xb1, p1);
    ec_ram_write(0xb2, p2);
    ec_ram_write(0xbf, 0x10);
    thread::sleep(Duration::from_millis(10));
    ec_ram_write(0xbf, 0xff);
    thread::sleep(Duration::from_millis(10));
}

// TODO: make sure it has proper values from /proc/ioports
const EC_CMD_PORT: u16 = 0x66;
const EC_DATA_PORT: u16 = 0x62;

const EC_IBF: u8 = 0b01;
const EC_OBF: u8 = 0b10;

const WR_EC: u8 = 0x81;

const TIMEOUT: Duration = Duration::from_secs(1);
const AIR_EC_RAM_BASE: u64 = 0xFE800400;
const AIR_EC_RAM_SIZE: usize = 0xFF;

enum EcRamAccess {
    IoPort,
    DevMem(MmapMut),
}

static EC_RAM_METHOD: Lazy<Mutex<EcRamAccess>> = Lazy::new(|| {
    let vendor = fs::read_to_string("/sys/class/dmi/id/board_vendor").unwrap_or("asdf".into());
    let name = fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or("asdf".into());
    let is_aya_air = vendor.trim() == "AYANEO" && name.trim().contains("AIR");

    if is_aya_air {
        eprintln!("Using fast-path EC RAM RW for Aya Neo Air");
        match OpenOptions::new().read(true).write(true).create(true).open("/dev/mem") {
            Err(e) => {
                eprintln!("Failed to open /dev/mem");
                eprintln!("Due to: {}", e);
                eprintln!("Falling back to I/O Port for EC RAM RW");
                Mutex::new(EcRamAccess::IoPort)
            },
            Ok(f) => {
                match unsafe { MmapOptions::new().offset(AIR_EC_RAM_BASE).len(AIR_EC_RAM_SIZE).map_mut(&f) } {
                    Ok(map) => Mutex::new(EcRamAccess::DevMem(map)),
                    Err(e) => {
                        eprintln!("Failed to mmap /dev/mem");
                        eprintln!("Due to: {}", e);
                        eprintln!("Falling back to I/O Port for EC RAM RW");
                        Mutex::new(EcRamAccess::IoPort)
                    }
                }
            }
        }
    } else {
        Mutex::new(EcRamAccess::IoPort)
    }
});

fn ec_ram_write(addr: u8, data: u8) {
    match EC_RAM_METHOD.lock().unwrap().deref_mut() {
        EcRamAccess::IoPort => {
            send_ec_command(WR_EC);
            send_ec_data(addr);
            send_ec_data(data);
        },
        EcRamAccess::DevMem(map) => {
            map[addr as usize] = data;
        },
    }
}

fn send_ec_command(command: u8) {
    block_until_ec_free();
    outb(command, EC_CMD_PORT);
}

fn send_ec_data(data: u8) {
    block_until_ec_free();
    outb(data, EC_DATA_PORT);
}

fn block_until_ec_free() {
    let start = Instant::now();
    while start.elapsed() < TIMEOUT && inb(EC_CMD_PORT) & EC_IBF != 0x0 {
        thread::sleep(Duration::from_millis(1));
        print!(".");
    }
    if start.elapsed() > TIMEOUT {
        eprintln!("Timed out waiting for EC's input buffer to have free space");
    }
}

fn outb(data: u8, port: u16) {
    //println!("sending 0x{:x} to port 0x{:x}", data, port);
    unsafe { asm!("out dx, al", in("dx") port, in("al") data, options(nostack)) }
}

fn inb(port: u16) -> u8 {
    let ret: u8;
    unsafe { asm!("in al, dx", out("al") ret, in("dx") port, options(nostack)) }
    ret
}

fn cap_to_rgb(capacity: u8) -> (u8, u8, u8) {
    match capacity {
        0..=20 => (255, 0, 0),
        90..=100 => (0, 0, 255),
        _ => (0, 0, 0),
    }
}

fn status_to_rgb(status: &str, capacity: u8) -> Option<(u8, u8, u8)> {
    match status.trim() {
        "Charging" => if capacity < 90 { Some((0, 255, 0)) } else { None },
        _ => None,
    }
}

fn main() {
    if unsafe { iopl(3) } != 0 {
        panic!("You must be root to run this utility");
    }

    // enable our control over those leds
    ec_cmd(0x03, 0x02, 0xc0);

    // find battery
    let battery_dir = fs::read_dir("/sys/class/power_supply").expect("Failed to open /sys/class/power_supply")
        .flatten()
        .find(|ps| {
            let mut path = ps.path();
            path.push("type");
            fs::read_to_string(path).unwrap_or("asdf".into()).trim() == "Battery"
        })
        .map(|dir| dir.path())
        .expect("Failed to find battery");

    let battery_cap_path = { let mut tmp = battery_dir.clone(); tmp.push("capacity"); tmp };
    let battery_status_path = { let mut tmp = battery_dir.clone(); tmp.push("status"); tmp };

    println!("Found battery at {:?}", &battery_dir);
    loop {
        let capacity = fs::read_to_string(&battery_cap_path).expect("Failed to read battery capacity").trim().parse::<u8>().unwrap_or(0);
        let status = fs::read_to_string(&battery_status_path).expect("Failed to read battery status");
        if let Some(color) = status_to_rgb(&status, capacity) {
            set_all_pixels(color);
        } else {
            let color = cap_to_rgb(capacity);
            set_all_pixels(color);
        }
        thread::sleep(Duration::from_millis(500));
    }
}
