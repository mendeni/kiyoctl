use anyhow::{anyhow, bail, Context, Result};
use eframe::egui;
use rusb::{DeviceHandle, GlobalContext};
use std::time::Duration;

const VID: u16 = 0x1532;
const PID: u16 = 0x0e05;
const TIMEOUT: Duration = Duration::from_millis(500);

// UVC class-specific request codes
const UVC_SET_CUR: u8 = 0x01;
const UVC_GET_CUR: u8 = 0x81;
const UVC_GET_MIN: u8 = 0x82;
const UVC_GET_MAX: u8 = 0x83;
const UVC_GET_RES: u8 = 0x84;
const UVC_GET_DEF: u8 = 0x87;

const REQ_SET: u8 = 0x21; // class | interface | host-to-device
const REQ_GET: u8 = 0xa1; // class | interface | device-to-host

// Razer EU1 extension unit, GUID 23e49ed0-1178-4f31-ae52-d2fb8a8d3b48
const EU1_SET_ISP: u8 = 0x01;
const XU_GUID: [u8; 16] = [
    0xd0, 0x9e, 0xe4, 0x23, 0x78, 0x11, 0x31, 0x4f, 0xae, 0x52, 0xd2, 0xfb, 0x8a, 0x8d, 0x3b,
    0x48,
];

const AF_RESPONSIVE: [u8; 8] = [0xff, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const AF_PASSIVE: [u8; 8] = [0xff, 0x06, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
const HDR_OFF: [u8; 8] = [0xff, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const HDR_ON: [u8; 8] = [0xff, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
const HDR_DARK: [u8; 8] = [0xff, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const HDR_BRIGHT: [u8; 8] = [0xff, 0x07, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00];
const FOV_WIDE: [u8; 8] = [0xff, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00];
const FOV_MEDIUM_PRE: [u8; 8] = [0xff, 0x01, 0x00, 0x03, 0x01, 0x00, 0x00, 0x00];
const FOV_MEDIUM: [u8; 8] = [0xff, 0x01, 0x01, 0x03, 0x01, 0x00, 0x00, 0x00];
const FOV_NARROW_PRE: [u8; 8] = [0xff, 0x01, 0x00, 0x03, 0x02, 0x00, 0x00, 0x00];
const FOV_NARROW: [u8; 8] = [0xff, 0x01, 0x01, 0x03, 0x02, 0x00, 0x00, 0x00];
// Persists current settings to camera NVRAM (what Synapse sends on save)
const SAVE: [u8; 8] = [0xc0, 0x03, 0xa8, 0x00, 0x00, 0x00, 0x00, 0x00];

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Processing,
    CameraTerminal,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Slider,
    Bool,
    Menu(&'static [(i64, &'static str)]),
    // menu whose supported entries come from GET_RES bitmask
    AeMenu,
}

struct Spec {
    name: &'static str,
    key: &'static str,
    section: &'static str,
    unit: Unit,
    selector: u8,
    len: usize,
    signed: bool,
    kind: Kind,
}

const POWERLINE_MENU: &[(i64, &str)] = &[(0, "Disabled"), (1, "50 Hz"), (2, "60 Hz")];
const AE_MODES: &[(i64, &str)] = &[
    (1, "Manual"),
    (2, "Auto"),
    (4, "Shutter priority"),
    (8, "Aperture priority"),
];

const SPECS: &[Spec] = &[
    Spec { name: "Brightness", key: "brightness", section: "Image", unit: Unit::Processing, selector: 0x02, len: 2, signed: true, kind: Kind::Slider },
    Spec { name: "Contrast", key: "contrast", section: "Image", unit: Unit::Processing, selector: 0x03, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Saturation", key: "saturation", section: "Image", unit: Unit::Processing, selector: 0x07, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Sharpness", key: "sharpness", section: "Image", unit: Unit::Processing, selector: 0x08, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Gamma", key: "gamma", section: "Image", unit: Unit::Processing, selector: 0x09, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Hue", key: "hue", section: "Image", unit: Unit::Processing, selector: 0x06, len: 2, signed: true, kind: Kind::Slider },
    Spec { name: "Backlight compensation", key: "backlight", section: "Image", unit: Unit::Processing, selector: 0x01, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Powerline frequency", key: "powerline", section: "Image", unit: Unit::Processing, selector: 0x05, len: 1, signed: false, kind: Kind::Menu(POWERLINE_MENU) },
    Spec { name: "Auto exposure", key: "ae_mode", section: "Exposure", unit: Unit::CameraTerminal, selector: 0x02, len: 1, signed: false, kind: Kind::AeMenu },
    Spec { name: "Exposure time", key: "exposure", section: "Exposure", unit: Unit::CameraTerminal, selector: 0x04, len: 4, signed: false, kind: Kind::Slider },
    Spec { name: "Gain", key: "gain", section: "Exposure", unit: Unit::Processing, selector: 0x04, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Auto white balance", key: "wb_auto", section: "White balance", unit: Unit::Processing, selector: 0x0b, len: 1, signed: false, kind: Kind::Bool },
    Spec { name: "WB temperature", key: "wb_temp", section: "White balance", unit: Unit::Processing, selector: 0x0a, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Autofocus", key: "focus_auto", section: "Focus & zoom", unit: Unit::CameraTerminal, selector: 0x08, len: 1, signed: false, kind: Kind::Bool },
    Spec { name: "Focus", key: "focus", section: "Focus & zoom", unit: Unit::CameraTerminal, selector: 0x06, len: 2, signed: false, kind: Kind::Slider },
    Spec { name: "Zoom", key: "zoom", section: "Focus & zoom", unit: Unit::CameraTerminal, selector: 0x0b, len: 2, signed: false, kind: Kind::Slider },
];

fn bytes_to_i64(b: &[u8], signed: bool) -> i64 {
    let mut v: u64 = 0;
    for (i, byte) in b.iter().enumerate() {
        v |= (*byte as u64) << (8 * i);
    }
    if signed && !b.is_empty() {
        let bits = 8 * b.len() as u32;
        let sign = 1u64 << (bits - 1);
        if v & sign != 0 {
            return v as i64 - (1i64 << bits);
        }
    }
    v as i64
}

fn i64_to_bytes(v: i64, len: usize) -> Vec<u8> {
    (v as u64).to_le_bytes()[..len].to_vec()
}

struct Camera {
    handle: DeviceHandle<GlobalContext>,
    vc_intf: u8,
    xu_id: u8,
    pu_id: u8,
    ct_id: u8,
    claimed: bool,
    detached: bool,
}

impl Camera {
    fn open() -> Result<Camera> {
        let device = rusb::devices()
            .context("USB enumeration failed")?
            .iter()
            .find(|d| {
                d.device_descriptor()
                    .map(|dd| dd.vendor_id() == VID && dd.product_id() == PID)
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow!("Razer Kiyo Pro not found on USB"))?;

        let handle = device
            .open()
            .context("could not open device (try running with sudo)")?;

        let mut hdr = [0u8; 9];
        handle
            .read_control(0x80, 0x06, 0x0200, 0, &mut hdr, TIMEOUT)
            .context("reading config descriptor header")?;
        let total = u16::from_le_bytes([hdr[2], hdr[3]]) as usize;
        let mut buf = vec![0u8; total];
        let n = handle
            .read_control(0x80, 0x06, 0x0200, 0, &mut buf, TIMEOUT)
            .context("reading full config descriptor")?;
        buf.truncate(n);

        let mut cam = Camera {
            handle,
            vc_intf: 0,
            xu_id: 0,
            pu_id: 0,
            ct_id: 0,
            claimed: false,
            detached: false,
        };
        cam.parse_descriptors(&buf);
        if cam.xu_id == 0 && cam.pu_id == 0 && cam.ct_id == 0 {
            bail!("no UVC control units found in descriptors");
        }
        Ok(cam)
    }

    fn parse_descriptors(&mut self, d: &[u8]) {
        let mut i = 0usize;
        let mut in_vc = false;
        while i + 2 <= d.len() {
            let len = d[i] as usize;
            if len < 2 || i + len > d.len() {
                break;
            }
            let body = &d[i..i + len];
            match body[1] {
                0x04 => {
                    // interface descriptor: number, class, subclass
                    in_vc = body[5] == 0x0e && body[6] == 0x01;
                    if in_vc {
                        self.vc_intf = body[2];
                    }
                }
                0x24 if in_vc => match body[2] {
                    0x02 if len >= 6 => {
                        // input terminal; 0x0201 = ITT_CAMERA
                        if u16::from_le_bytes([body[4], body[5]]) == 0x0201 {
                            self.ct_id = body[3];
                        }
                    }
                    0x05 if len >= 4 => self.pu_id = body[3],
                    0x06 if len >= 20 => {
                        if body[4..20] == XU_GUID {
                            self.xu_id = body[3];
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            i += len;
        }
    }

    fn unit_id(&self, unit: Unit) -> u8 {
        match unit {
            Unit::Processing => self.pu_id,
            Unit::CameraTerminal => self.ct_id,
        }
    }

    fn try_claim(&mut self) -> Result<()> {
        let _ = self.handle.set_auto_detach_kernel_driver(true);
        if let Ok(true) = self.handle.kernel_driver_active(self.vc_intf) {
            if self.handle.detach_kernel_driver(self.vc_intf).is_ok() {
                self.detached = true;
            }
        }
        self.handle
            .claim_interface(self.vc_intf)
            .context("claiming VideoControl interface failed (try sudo)")?;
        self.claimed = true;
        Ok(())
    }

    fn get(&mut self, req: u8, unit_id: u8, selector: u8, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let value = (selector as u16) << 8;
        let index = ((unit_id as u16) << 8) | self.vc_intf as u16;
        match self.handle.read_control(REQ_GET, req, value, index, &mut buf, TIMEOUT) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(rusb::Error::Pipe) => Err(anyhow!("unsupported")),
            Err(e) if !self.claimed => {
                self.try_claim()?;
                let n = self
                    .handle
                    .read_control(REQ_GET, req, value, index, &mut buf, TIMEOUT)
                    .with_context(|| format!("GET {req:#x} failed after claim: {e}"))?;
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) => Err(e.into()),
        }
    }

    fn set(&mut self, unit_id: u8, selector: u8, data: &[u8]) -> Result<()> {
        let value = (selector as u16) << 8;
        let index = ((unit_id as u16) << 8) | self.vc_intf as u16;
        match self.handle.write_control(REQ_SET, UVC_SET_CUR, value, index, data, TIMEOUT) {
            Ok(_) => Ok(()),
            Err(e) if !self.claimed && e != rusb::Error::Pipe => {
                self.try_claim()?;
                self.handle
                    .write_control(REQ_SET, UVC_SET_CUR, value, index, data, TIMEOUT)
                    .with_context(|| format!("SET_CUR failed after claim: {e}"))?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn xu_send(&mut self, data: &[u8; 8]) -> Result<()> {
        if self.xu_id == 0 {
            bail!("Razer extension unit not found");
        }
        self.set(self.xu_id, EU1_SET_ISP, data)
    }

    fn save_to_nvram(&mut self) -> Result<()> {
        self.xu_send(&SAVE)
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        if self.claimed {
            let _ = self.handle.release_interface(self.vc_intf);
        }
        if self.detached {
            let _ = self.handle.attach_kernel_driver(self.vc_intf);
        }
    }
}

struct CtrlState {
    spec: &'static Spec,
    min: i64,
    max: i64,
    def: i64,
    cur: i64,
    ae_mask: u8,
}

fn read_ctrls(cam: &mut Camera) -> Vec<CtrlState> {
    let mut out = Vec::new();
    for spec in SPECS {
        let unit_id = cam.unit_id(spec.unit);
        if unit_id == 0 {
            continue;
        }
        let cur = match cam.get(UVC_GET_CUR, unit_id, spec.selector, spec.len) {
            Ok(b) => bytes_to_i64(&b, spec.signed),
            Err(_) => continue, // control not supported by this camera
        };
        let mut st = CtrlState { spec, min: 0, max: 1, def: 0, cur, ae_mask: 0xff };
        if spec.kind == Kind::Slider {
            let Ok(minb) = cam.get(UVC_GET_MIN, unit_id, spec.selector, spec.len) else { continue };
            let Ok(maxb) = cam.get(UVC_GET_MAX, unit_id, spec.selector, spec.len) else { continue };
            st.min = bytes_to_i64(&minb, spec.signed);
            st.max = bytes_to_i64(&maxb, spec.signed);
            st.def = cam
                .get(UVC_GET_DEF, unit_id, spec.selector, spec.len)
                .map(|b| bytes_to_i64(&b, spec.signed))
                .unwrap_or(st.cur);
        }
        if spec.kind == Kind::AeMenu {
            if let Ok(b) = cam.get(UVC_GET_RES, unit_id, spec.selector, spec.len) {
                if !b.is_empty() && b[0] != 0 {
                    st.ae_mask = b[0];
                }
            }
        }
        out.push(st);
    }
    out
}

fn apply(cam: &mut Camera, spec: &Spec, value: i64) -> Result<()> {
    let unit_id = cam.unit_id(spec.unit);
    cam.set(unit_id, spec.selector, &i64_to_bytes(value, spec.len))
}

// ---------- Kiyo XU (set-only; current values are not readable) ----------

struct XuCtrl {
    name: &'static str,
    key: &'static str,
    options: &'static [(&'static str, &'static [u8; 8], Option<&'static [u8; 8]>)],
}

const XU_CTRLS: &[XuCtrl] = &[
    XuCtrl { name: "HDR", key: "hdr", options: &[("off", &HDR_OFF, None), ("on", &HDR_ON, None)] },
    XuCtrl { name: "HDR mode", key: "hdr_mode", options: &[("bright", &HDR_BRIGHT, None), ("dark", &HDR_DARK, None)] },
    XuCtrl { name: "Autofocus mode", key: "af_mode", options: &[("passive", &AF_PASSIVE, None), ("responsive", &AF_RESPONSIVE, None)] },
    XuCtrl { name: "Field of view", key: "fov", options: &[
        ("wide", &FOV_WIDE, None),
        ("medium", &FOV_MEDIUM, Some(&FOV_MEDIUM_PRE)),
        ("narrow", &FOV_NARROW, Some(&FOV_NARROW_PRE)),
    ] },
];

fn xu_apply(cam: &mut Camera, ctrl: &XuCtrl, opt_idx: usize) -> Result<()> {
    let (_, payload, pre) = ctrl.options[opt_idx];
    if let Some(pre) = pre {
        cam.xu_send(pre)?;
    }
    cam.xu_send(payload)
}

// ---------- CLI ----------

fn cli_probe() -> Result<()> {
    let mut cam = Camera::open()?;
    println!(
        "Kiyo Pro found. VC interface {}, XU id {}, PU id {}, CT id {}",
        cam.vc_intf, cam.xu_id, cam.pu_id, cam.ct_id
    );
    for st in read_ctrls(&mut cam) {
        match st.spec.kind {
            Kind::Slider => println!(
                "  {:<12} = {:<6} (min {} max {} default {})",
                st.spec.key, st.cur, st.min, st.max, st.def
            ),
            _ => println!("  {:<12} = {}", st.spec.key, st.cur),
        }
    }
    println!("  XU set-only controls: hdr, hdr_mode, af_mode, fov (+ `save`)");
    Ok(())
}

fn cli_set(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("usage: kiyoctl set key=value [key=value ...]");
    }
    let mut cam = Camera::open()?;
    for arg in args {
        let (key, val) = arg
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid argument {arg}, expected key=value"))?;
        if let Some(ctrl) = XU_CTRLS.iter().find(|c| c.key == key) {
            let idx = ctrl
                .options
                .iter()
                .position(|(name, _, _)| *name == val)
                .ok_or_else(|| {
                    let opts: Vec<_> = ctrl.options.iter().map(|(n, _, _)| *n).collect();
                    anyhow!("{key}: unknown value {val}, expected one of {opts:?}")
                })?;
            xu_apply(&mut cam, ctrl, idx)?;
            println!("{key}={val}");
        } else if let Some(spec) = SPECS.iter().find(|s| s.key == key) {
            let v: i64 = val.parse().with_context(|| format!("{key}: {val} is not a number"))?;
            apply(&mut cam, spec, v)?;
            println!("{key}={v}");
        } else {
            bail!("unknown control {key}; run `kiyoctl probe` to list controls");
        }
    }
    Ok(())
}

fn cli_save() -> Result<()> {
    let mut cam = Camera::open()?;
    cam.save_to_nvram()?;
    println!("save-to-NVRAM sent; replug the camera to verify settings persisted");
    Ok(())
}

// ---------- Live preview (AVFoundation capture on a background thread) ----------

struct PreviewShared {
    frame: std::sync::Mutex<Option<egui::ColorImage>>,
    status: std::sync::Mutex<String>,
    stop: std::sync::atomic::AtomicBool,
}

struct Preview {
    shared: std::sync::Arc<PreviewShared>,
    texture: Option<egui::TextureHandle>,
}

impl Preview {
    fn start(ctx: egui::Context) -> Preview {
        let shared = std::sync::Arc::new(PreviewShared {
            frame: std::sync::Mutex::new(None),
            status: std::sync::Mutex::new("Starting preview…".into()),
            stop: std::sync::atomic::AtomicBool::new(false),
        });
        let thread_shared = shared.clone();
        std::thread::spawn(move || capture_loop(thread_shared, ctx));
        Preview { shared, texture: None }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(img) = self.shared.frame.lock().unwrap().take() {
            match &mut self.texture {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ui.ctx().load_texture("preview", img, egui::TextureOptions::LINEAR))
                }
            }
        }
        match &self.texture {
            Some(tex) => {
                ui.add(
                    egui::Image::new(tex)
                        .max_width(ui.available_width())
                        .maintain_aspect_ratio(true),
                );
            }
            None => {
                ui.label(self.shared.status.lock().unwrap().clone());
            }
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.shared.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn capture_loop(shared: std::sync::Arc<PreviewShared>, ctx: egui::Context) {
    use std::sync::atomic::Ordering;

    // macOS camera permission: request and wait for the user's answer
    if !nokhwa::nokhwa_check() {
        nokhwa::nokhwa_initialize(|_| {});
        for _ in 0..100 {
            if nokhwa::nokhwa_check() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    while !shared.stop.load(Ordering::Relaxed) {
        if let Err(e) = run_capture(&shared, &ctx) {
            *shared.status.lock().unwrap() = format!("Preview: {e:#} — retrying…");
            ctx.request_repaint();
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

fn run_capture(shared: &PreviewShared, ctx: &egui::Context) -> Result<()> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{
        ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
        Resolution,
    };
    use std::sync::atomic::Ordering;

    let devices = nokhwa::query(ApiBackend::AVFoundation).context("camera enumeration")?;
    let index = devices
        .iter()
        .find(|d| d.human_name().to_lowercase().contains("kiyo"))
        .map(|d| d.index().clone())
        .unwrap_or(CameraIndex::Index(0));

    // the camera may not offer the exact format we'd prefer, so fall back progressively
    let attempts = [
        RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::NV12,
            30,
        )),
        RequestedFormatType::Closest(CameraFormat::new(
            Resolution::new(1280, 720),
            FrameFormat::YUYV,
            30,
        )),
        RequestedFormatType::AbsoluteHighestFrameRate,
        RequestedFormatType::AbsoluteHighestResolution,
        RequestedFormatType::None,
    ];
    let mut cam = None;
    let mut last_err = None;
    for fmt in attempts {
        match nokhwa::Camera::new(index.clone(), RequestedFormat::new::<RgbFormat>(fmt)) {
            Ok(c) => {
                cam = Some(c);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let mut cam = cam.ok_or_else(|| {
        anyhow!("opening capture device: {}", last_err.map(|e| e.to_string()).unwrap_or_default())
    })?;
    cam.open_stream().context("starting stream")?;
    *shared.status.lock().unwrap() = format!("Preview: {}", cam.info().human_name());

    while !shared.stop.load(Ordering::Relaxed) {
        let frame = cam.frame().context("reading frame")?;
        let decoded = frame.decode_image::<RgbFormat>().context("decoding frame")?;
        let size = [decoded.width() as usize, decoded.height() as usize];
        let img = egui::ColorImage::from_rgb(size, decoded.as_raw());
        *shared.frame.lock().unwrap() = Some(img);
        ctx.request_repaint();
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}

// ---------- GUI ----------

struct App {
    cam: Option<Camera>,
    ctrls: Vec<CtrlState>,
    xu_selected: Vec<Option<usize>>,
    status: String,
    preview: Preview,
}

impl App {
    fn new(ctx: egui::Context) -> App {
        let mut app = App {
            cam: None,
            ctrls: Vec::new(),
            xu_selected: vec![None; XU_CTRLS.len()],
            status: String::new(),
            preview: Preview::start(ctx),
        };
        app.connect();
        app
    }

    fn connect(&mut self) {
        match Camera::open() {
            Ok(mut cam) => {
                self.ctrls = read_ctrls(&mut cam);
                self.status = format!("Connected ({} controls)", self.ctrls.len());
                self.cam = Some(cam);
            }
            Err(e) => {
                self.cam = None;
                self.ctrls.clear();
                self.status = format!("{e:#}");
            }
        }
    }

    fn cur(&self, key: &str) -> Option<i64> {
        self.ctrls.iter().find(|c| c.spec.key == key).map(|c| c.cur)
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Kiyo Pro");
                if ui.button("Reconnect / refresh").clicked() {
                    self.connect();
                }
                let can_save = self.cam.is_some();
                if ui
                    .add_enabled(can_save, egui::Button::new("💾 Save to camera"))
                    .on_hover_text("Persist current settings to the camera's onboard memory")
                    .clicked()
                {
                    let res = self.cam.as_mut().unwrap().save_to_nvram();
                    self.status = match res {
                        Ok(()) => "Saved to camera NVRAM — replug to verify".into(),
                        Err(e) => format!("Save failed: {e:#}"),
                    };
                }
            });
            ui.label(&self.status);
            ui.separator();
            self.preview.ui(ui);
            ui.separator();

            if self.cam.is_none() {
                ui.label("Camera not connected. Plug in the Kiyo Pro and hit Reconnect.");
                ui.label("If access is denied, run from a terminal with: sudo kiyoctl");
                return;
            }

            let wb_auto = self.cur("wb_auto") == Some(1);
            let ae_auto = matches!(self.cur("ae_mode"), Some(2) | Some(8));
            let focus_auto = self.cur("focus_auto") == Some(1);

            let mut pending: Vec<(usize, i64)> = Vec::new();
            let mut section = "";
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, st) in self.ctrls.iter_mut().enumerate() {
                    if st.spec.section != section {
                        section = st.spec.section;
                        ui.add_space(8.0);
                        ui.strong(section);
                    }
                    let enabled = match st.spec.key {
                        "wb_temp" => !wb_auto,
                        "exposure" => !ae_auto,
                        "focus" => !focus_auto,
                        _ => true,
                    };
                    let mut val = st.cur;
                    let changed = match st.spec.kind {
                        Kind::Slider => ui
                            .horizontal(|ui| {
                                ui.add_enabled_ui(enabled, |ui| {
                                    let r = ui.add(
                                        egui::Slider::new(&mut val, st.min..=st.max)
                                            .text(st.spec.name),
                                    );
                                    let reset = ui
                                        .small_button("↺")
                                        .on_hover_text(format!("Reset to default ({})", st.def))
                                        .clicked();
                                    if reset {
                                        val = st.def;
                                    }
                                    r.changed() || reset
                                })
                                .inner
                            })
                            .inner,
                        Kind::Bool => {
                            let mut b = val == 1;
                            let r = ui.checkbox(&mut b, st.spec.name);
                            val = if b { 1 } else { 0 };
                            r.changed()
                        }
                        Kind::Menu(options) => {
                            let mut changed = false;
                            let current = options
                                .iter()
                                .find(|(v, _)| *v == val)
                                .map(|(_, n)| *n)
                                .unwrap_or("?");
                            egui::ComboBox::from_label(st.spec.name)
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    for (v, name) in options {
                                        if ui.selectable_value(&mut val, *v, *name).changed() {
                                            changed = true;
                                        }
                                    }
                                });
                            changed
                        }
                        Kind::AeMenu => {
                            // options filtered by the camera's supported-modes bitmask
                            let mut changed = false;
                            let current = AE_MODES
                                .iter()
                                .find(|(v, _)| *v == val)
                                .map(|(_, n)| *n)
                                .unwrap_or("?");
                            egui::ComboBox::from_label(st.spec.name)
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    for (v, name) in AE_MODES {
                                        if st.ae_mask & (*v as u8) == 0 {
                                            continue;
                                        }
                                        if ui.selectable_value(&mut val, *v, *name).changed() {
                                            changed = true;
                                        }
                                    }
                                });
                            changed
                        }
                    };
                    if changed && val != st.cur {
                        st.cur = val;
                        pending.push((i, val));
                    }
                }

                ui.add_space(8.0);
                ui.strong("Kiyo Pro (set-only)");
                ui.label("The camera can't report these; pick a value to send it.");
                for (i, ctrl) in XU_CTRLS.iter().enumerate() {
                    let mut sel = self.xu_selected[i];
                    let text = sel.map(|s| ctrl.options[s].0).unwrap_or("—");
                    let mut chosen: Option<usize> = None;
                    egui::ComboBox::from_label(ctrl.name)
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            for (j, (name, _, _)) in ctrl.options.iter().enumerate() {
                                if ui.selectable_value(&mut sel, Some(j), *name).changed() {
                                    chosen = Some(j);
                                }
                            }
                        });
                    if let Some(j) = chosen {
                        self.xu_selected[i] = Some(j);
                        let res = xu_apply(self.cam.as_mut().unwrap(), ctrl, j);
                        self.status = match res {
                            Ok(()) => format!("{} = {}", ctrl.name, ctrl.options[j].0),
                            Err(e) => format!("{}: {e:#}", ctrl.name),
                        };
                    }
                }
            });

            for (i, val) in pending {
                let spec = self.ctrls[i].spec;
                let res = apply(self.cam.as_mut().unwrap(), spec, val);
                self.status = match res {
                    Ok(()) => format!("{} = {}", spec.name, val),
                    Err(e) => format!("{}: {e:#}", spec.name),
                };
            }
        });
    }
}

fn run_gui() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([480.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Kiyo Pro Controls",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc.egui_ctx.clone())))),
    )
    .map_err(|e| anyhow!("gui error: {e}"))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        None => run_gui(),
        Some("probe") => cli_probe(),
        Some("set") => cli_set(&args[1..]),
        Some("save") => cli_save(),
        Some(other) => bail!(
            "unknown command {other}\nusage: kiyoctl [probe | set key=value ... | save]\n(no arguments launches the GUI)"
        ),
    }
}
