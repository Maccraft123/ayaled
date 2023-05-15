use std::thread;
use std::arch::asm;
use std::fs::{self, OpenOptions};
use std::time::Duration;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use rouille::Response;
use memmap::{MmapMut, MmapOptions};
use libc::iopl;

fn outb(port: u16, data: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") data, options(nostack)) }
}

struct AirPlusLedCtl();

impl AirPlusLedCtl {
    const ADDR_PORT: u16 = 0x4e;
    const DATA_PORT: u16 = 0x4f;

    fn cmd(&mut self, hi: u8, lo: u8, val: u8) {
        outb(Self::ADDR_PORT, 0x2e);
        outb(Self::DATA_PORT, 0x11);
        outb(Self::ADDR_PORT, 0x2f);
        outb(Self::DATA_PORT, hi);

        outb(Self::ADDR_PORT, 0x2e);
        outb(Self::DATA_PORT, 0x10);
        outb(Self::ADDR_PORT, 0x2f);
        outb(Self::DATA_PORT, lo);

        outb(Self::ADDR_PORT, 0x2e);
        outb(Self::DATA_PORT, 0x12);
        outb(Self::ADDR_PORT, 0x2f);
        outb(Self::DATA_PORT, val);
    }
}

impl LedCtl for AirPlusLedCtl {
    fn init() -> Self {
        let mut tmp = Self();
        tmp.cmd(0xd1, 0x87, 0xa5);
        tmp.cmd(0xd1, 0xb2, 0x31);
        tmp.cmd(0xd1, 0xc6, 0x01);

        tmp.cmd(0xd1, 0x87, 0xa5);
        tmp.cmd(0xd1, 0x72, 0x31);
        tmp.cmd(0xd1, 0x86, 0x01);

        tmp.cmd(0xd1, 0x87, 0xa5);
        tmp.cmd(0xd1, 0x70, 0x00);
        tmp.cmd(0xd1, 0x86, 0x01);
        tmp.cmd(0xd1, 0x60, 0x80);
        tmp
    }
    fn probe() -> bool {
        let vendor = fs::read_to_string("/sys/class/dmi/id/board_vendor").unwrap_or("asdf".into());
        let name = fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or("asdf".into());

        return vendor.trim() == "AYANEO" && name.trim() == "AIR Plus";
    }
}

struct AirLedCtl {
    map: MmapMut, 
}

impl AirLedCtl {
    const LEFT_JOYSTICK: u8 = 1;
    const RIGHT_JOYSTICK: u8 = 2;

    const RIGHT_LED: u8 = 1;
    const BOTTOM_LED: u8 = 2;
    const LEFT_LED: u8 = 3;
    const TOP_LED: u8 = 4;

    const AIR_EC_RAM_BASE: u64 = 0xFE800400;
    const AIR_EC_RAM_SIZE: usize = 0xFF;

    fn set_pixel(&mut self, js: u8, led: u8, color: (u8, u8, u8)) {
        self.ec_cmd(js, led * 3, color.0);
        self.ec_cmd(js, led * 3 + 1, color.1);
        self.ec_cmd(js, led * 3 + 2, color.2);
    }

    fn ec_cmd(&mut self, cmd: u8, p1: u8, p2: u8) {
        self.map[0x6d] = cmd;
        self.map[0xb1] = p1;
        self.map[0xb2] = p2;
        self.map[0xbf] = 0x10;
        thread::sleep(Duration::from_millis(10));
        self.map[0xbf] = 0xff;
        thread::sleep(Duration::from_millis(10));
    }
}

impl LedCtl for AirLedCtl {
    fn init() -> Self {
        let devmem = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("/dev/mem")
            .expect("Failed to open /dev/mem");

        unsafe {
            let map = MmapOptions::new()
                .offset(Self::AIR_EC_RAM_BASE)
                .len(Self::AIR_EC_RAM_SIZE)
                .map_mut(&devmem)
                .expect("Failed to mmap /dev/mem");

            let mut tmp = Self { map };
            tmp.ec_cmd(0x03, 0x02, 0x00);
            tmp
        }
    }
    fn probe() -> bool {
        let vendor = fs::read_to_string("/sys/class/dmi/id/board_vendor").unwrap_or("asdf".into());
        let name = fs::read_to_string("/sys/class/dmi/id/board_name").unwrap_or("asdf".into());
        let supported_devices: [&str; 4] = ["AIR", "AIR Pro", "AYANEO 2", "GEEK"];
        let is_supported = vendor.trim() == "AYANEO" && supported_devices.contains(&name.trim());

        is_supported
    }
    fn set_rgb(&mut self, color: (u8, u8, u8)) {
        self.set_pixel(Self::LEFT_JOYSTICK, Self::RIGHT_LED, color);
        self.set_pixel(Self::LEFT_JOYSTICK, Self::BOTTOM_LED, color);
        self.set_pixel(Self::LEFT_JOYSTICK, Self::LEFT_LED, color);
        self.set_pixel(Self::LEFT_JOYSTICK, Self::TOP_LED, color);

        self.set_pixel(Self::RIGHT_JOYSTICK, Self::RIGHT_LED, color);
        self.set_pixel(Self::RIGHT_JOYSTICK, Self::BOTTOM_LED, color);
        self.set_pixel(Self::RIGHT_JOYSTICK, Self::LEFT_LED, color);
        self.set_pixel(Self::RIGHT_JOYSTICK, Self::TOP_LED, color);
    }
    fn supports_rgb(&self) -> bool {
        true
    }
}

trait LedCtl {
    fn init() -> Self where Self: Sized;
    fn probe() -> bool where Self: Sized;
    fn set_rgb(&mut self, _rgb: (u8, u8, u8)) {}
    fn supports_rgb(&self) -> bool { false }
}

fn get_led_controller() -> Box<dyn LedCtl> {
    if AirLedCtl::probe() {
        println!("Using AIR LED controller");
        Box::new(AirLedCtl::init())
    } else if AirPlusLedCtl::probe() {
        println!("Using AIR Plus LED controller");
        Box::new(AirPlusLedCtl::init())
    } else {
        panic!("This device is not supported in ayaled")
    }
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

    let mut led_ctl = get_led_controller();
    if !led_ctl.supports_rgb() {
        println!("This device doesn't support setting RGB values. quitting");
        return;
    }

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
            led_ctl.set_rgb(adjusted_color);
            old = adjusted_color;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
