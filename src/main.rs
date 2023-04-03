use std::thread;
use std::fs::{self, OpenOptions};
use std::time::{Instant, Duration};
use std::arch::asm;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::ops::DerefMut;
use rouille::Response;
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
    let supported_devices: [&str; 4] = ["AIR", "AIR Pro", "AYANEO 2", "GEEK"];
    let is_supported = vendor.trim() == "AYANEO" && supported_devices.contains(&name.trim());

    if !is_supported {
        panic!("Not running on a supported device");
    }
    eprintln!("Using fast-path EC RAM RW.");
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

struct Theme {
    charging: (u8, u8, u8),
    low_bat: (u8, u8, u8),
    full: (u8, u8, u8),
    normal: (u8, u8, u8),
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            charging: (0, 0, 255),
            low_bat: (255, 0, 0),
            full: (0, 255, 255),
            normal: (0, 0, 0),
        }
    }
}

fn http_thread(theme: Arc<Mutex<Theme>>) {
    rouille::start_server("127.0.0.1:21371", move |request| {
        rouille::router!(request,
            (GET) (/set/{mode: String}/{r: u8}/{g: u8}/{b:u8}) => {
                let mut t = theme.lock().unwrap();
                match mode.as_str() {
                    "charging" => t.charging = (r, g, b),
                    "low_bat" => t.low_bat = (r, g, b),
                    "full" => t.full = (r, g, b),
                    "normal" => t.normal = (r, g, b),
                    _ => return Response::empty_400(),
                }
                drop(t);
                Response::empty_204()
            },
            (GET) (/get/{mode: String}) => {
                let t = theme.lock().unwrap();
                let data = match mode.as_str() {
                    "charging" => &t.charging,
                    "low_bat" => &t.low_bat,
                    "full" => &t.full,
                    "normal" => &t.normal,
                    _ => return Response::empty_400(),
                };
                let response = Response::text(format!("{}:{}:{}\n", data.0, data.1, data.2));
                drop(t);
                response
            },
            _ => Response::empty_404()
        )
    });
}

fn suspend_watcher() {
    let kern_entries = rmesg::logs_iter(rmesg::Backend::Default, false, false)
        .expect("Failed to init kernel log iter");
    for maybe_entry in kern_entries {
        if let Ok(entry) = maybe_entry {
            if entry.message.contains("PM: suspend exit") {
                JUST_RESUMED.store(true, Ordering::SeqCst);
            }
        }
    }
}

fn get_brightness_normalized() -> Option<f32> {
    let backlight_dir = fs::read_dir("/sys/class/backlight").ok()?
        .flatten()
        .map(|entry| entry.path())
        .next()?;

    let brightness_file = { let mut tmp = backlight_dir.clone(); tmp.push("brightness"); tmp };
    let max_brightness_file = { let mut tmp = backlight_dir.clone(); tmp.push("max_brightness"); tmp };


    let brightness = fs::read_to_string(&brightness_file)
        .expect("Failed to read backlight brightness")
        .trim()
        .parse::<f32>()
        .unwrap_or(1.0);
    let max_brightness = fs::read_to_string(&max_brightness_file)
        .expect("Failed to read maximum backlight brightness")
        .trim()
        .parse::<f32>()
        .unwrap_or(1.0);

    Some(brightness / max_brightness)
}

static JUST_RESUMED: AtomicBool = AtomicBool::new(false);

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

    let theme_mutex = Arc::new(Mutex::new(Theme::default()));
    let theme_mutex_2 = Arc::clone(&theme_mutex);
    thread::spawn(|| http_thread(theme_mutex_2));
    thread::spawn(|| suspend_watcher());

    println!("Found battery at {:?}", &battery_dir);
    let mut old = (0, 0, 0);
    loop {
        let capacity = fs::read_to_string(&battery_cap_path).expect("Failed to read battery capacity").trim().parse::<u8>().unwrap_or(0);
        let status = fs::read_to_string(&battery_status_path).expect("Failed to read battery status");
        let theme = theme_mutex.lock().unwrap();
        let color = match status.trim() {
            "Charging" => if capacity < 90 {
                theme.charging
            } else {
                theme.full
            },
            _ => match capacity {
                0..=20 => theme.low_bat,
                90..=100 => theme.full,
                _ => theme.normal,
            },
        };
        drop(theme);

        let scale = get_brightness_normalized().unwrap_or(1.0);
        let tmp = (color.0 as f32 * scale, color.1 as f32 * scale, color.2 as f32 * scale);
        let adjusted_color = (tmp.0 as u8, tmp.1 as u8, tmp.2 as u8);
        let force_set = JUST_RESUMED.swap(false, Ordering::SeqCst);

        if old != adjusted_color || force_set {
            set_all_pixels(adjusted_color);
            old = adjusted_color;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
