//! GPU desktop capture + downscale for the minimap (`--map`).
//!
//! The CPU path (`main::resample_desktop`) pulls the whole desktop into memory
//! with `CreateDCW("DISPLAY")` and then resamples it with `StretchBlt(HALFTONE)`.
//! On this machine that is a large slice of the minimap process's CPU, because
//! the HALFTONE filter walks every source pixel on the CPU.
//!
//! Everything here keeps the pixels on the GPU instead:
//!
//!   IDXGIOutputDuplication  (Desktop Duplication API, one per monitor)
//!     -> a D3D11 texture holding that monitor's framebuffer
//!     -> one pixel-shader pass that box-filters the monitor straight down to
//!        its own slot inside the minimap (4x4 taps, so a 1920px screen is not
//!        point-sampled every 6th pixel into a 360px thumbnail)
//!     -> a single small readback: only the minimap-sized result leaves the GPU
//!
//! Frames arrive exactly when the desktop presented one, which is what "the
//! refresh rate follows the desktop" means here: no timer, no polling. Monitors
//! are drawn at their real position in the virtual desktop, so a second screen
//! shows up next to the first at the right place.
//!
//! # Degradation chain
//!
//! Desktop Duplication does not work everywhere, so nothing here is assumed:
//!
//!   1. `D3D11CreateDevice(HARDWARE)`; if that fails, `D3D11CreateDevice(WARP)`;
//!      if that fails too, the GDI capture in `main::resample_desktop` stays.
//!   2. `DuplicateOutput` returning `DXGI_ERROR_UNSUPPORTED` (hybrid-graphics
//!      laptops, Optimus, RDP, virtual display adapters) drops straight to GDI:
//!      another device type cannot make an output duplicable.
//!   3. `DXGI_ERROR_ACCESS_LOST` at runtime (exclusive fullscreen, driver reset,
//!      secure desktop) rebuilds the duplication; more than three rebuilds in a
//!      row and the process stays on GDI for good.
//!   4. Every step is logged with `logline!` and the reason, never silently.
//!
//! `AI_MOUSE_MINIMAP_GPU` exists so the chain can be exercised on a machine that
//! has only one working path: `hardware` (default), `warp`, `gdi`, `lost`
//! (inject one ACCESS_LOST after 20 frames, to test the rebuild). It is logged
//! at startup.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout,
    ID3D11PixelShader, ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView,
    ID3D11Query, ID3D11Texture2D, ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_READ,
    D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
    D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_SAMPLER_DESC, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC,
    D3D11_USAGE_STAGING, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_ROTATION_IDENTITY, DXGI_SAMPLE_DESC,
};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIDevice, IDXGIFactory1, IDXGIOutput1,
    IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_INVALID_CALL,
    DXGI_ERROR_NOT_CURRENTLY_AVAILABLE, DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_ERROR_WAS_STILL_DRAWING, DXGI_OUTDUPL_FRAME_INFO,
};

/// One monitor as it sits in the virtual desktop (the same coordinate space as
/// `SM_XVIRTUALSCREEN`), captured through its own duplication object.
pub struct Monitor {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// CPU-side mirror of the last GPU-rendered thumbnail: BGRA, top-down, exactly
/// the size the caller asked for, so it can be memcpy'd into the minimap canvas
/// without any further scaling.
#[derive(Default)]
pub struct Thumb {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
    pub valid: bool,
}

/// Returned by `update` when DXGI lost the duplication. The capture has to be
/// rebuilt, but the GPU path itself is fine - the caller must not fall back
/// permanently on the first loss.
pub const ACCESS_LOST: &str = "DXGI_ERROR_ACCESS_LOST";

/// Which device type the capture runs on. WARP is a CPU rasterizer, so it is only
/// ever a fallback before GDI, never a performance improvement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Driver {
    Hardware,
    Warp,
}

impl Driver {
    fn label(self) -> &'static str {
        match self {
            Driver::Hardware => "hardware D3D11",
            Driver::Warp => "WARP software D3D11",
        }
    }
}

/// Why a build attempt failed. `Unsupported` must degrade immediately: retrying
/// with another device type cannot make an output duplicable.
enum Failure {
    Unsupported(String),
    Fatal(String),
}

/// Errors from `DuplicateOutput` that mean "this device cannot duplicate this
/// output", i.e. the hybrid-graphics / Optimus / RDP / virtual-adapter family.
///
/// Measured on this machine: a WARP device asked to duplicate a real display
/// output returns `E_INVALIDARG` (0x80070057), *not* `DXGI_ERROR_UNSUPPORTED`, so
/// keying only on UNSUPPORTED would leave the WARP rung retrying forever on some
/// drivers instead of dropping to GDI.
fn cannot_duplicate(code: windows::core::HRESULT) -> bool {
    code == DXGI_ERROR_UNSUPPORTED
        || code == DXGI_ERROR_NOT_CURRENTLY_AVAILABLE
        || code == DXGI_ERROR_INVALID_CALL
        || code == E_INVALIDARG
}

/// Constant buffer layout, mirrored by `Cb` below (48 bytes).
const HLSL: &str = r#"
cbuffer CB : register(b0) {
    float4 dst;    // x0, y0, x1, y1 of the destination rect in clip space
    float4 uv;     // u0, v0, du, dv of the source rect
    float2 foot;   // UV footprint of one destination pixel
    float2 pad;
};

struct VSOut {
    float4 pos : SV_POSITION;
    float2 t   : TEXCOORD0;
};

// One triangle big enough to cover the destination rect; the parts outside it
// are clipped, so a per-monitor draw costs three vertices and no vertex buffer.
VSOut VSMain(uint id : SV_VertexID) {
    float2 c = float2((id == 1) ? 2.0 : 0.0, (id == 2) ? 2.0 : 0.0);
    VSOut o;
    o.pos = float4(lerp(dst.x, dst.z, c.x), lerp(dst.y, dst.w, c.y), 0.0, 1.0);
    o.t = uv.xy + c * uv.zw;
    return o;
}

Texture2D    screen : register(t0);
SamplerState smp    : register(s0);

// 4x4 box filter over the source footprint of this destination pixel. Plain
// bilinear would point-sample the desktop and make text shimmer; this averages
// the area instead, which is also why a smaller minimap is simply blurrier.
float4 PSMain(VSOut i) : SV_Target {
    float4 acc = 0.0;
    [unroll] for (uint y = 0; y < 4; ++y) {
        [unroll] for (uint x = 0; x < 4; ++x) {
            float2 o = (float2(x, y) + 0.5) * 0.25 - 0.5;
            acc += screen.Sample(smp, i.t + o * foot);
        }
    }
    // the minimap canvas does its own alpha compositing: the thumbnail is opaque
    return float4(acc.rgb * 0.0625, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Cb {
    dst: [f32; 4],
    uv: [f32; 4],
    foot: [f32; 2],
    _pad: [f32; 2],
}

struct Output {
    dupl: IDXGIOutputDuplication,
    mon: Monitor,
    /// Copy of the last delivered frame. Duplication hands out a resource that
    /// cannot be sampled, and it is only valid until `ReleaseFrame`, so it goes
    /// through this texture first and its content stays valid between frames.
    frame: Option<ID3D11Texture2D>,
    srv: Option<ID3D11ShaderResourceView>,
    fw: u32,
    fh: u32,
    rotated: bool,
}

struct Capture {
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    driver: Driver,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    samp: ID3D11SamplerState,
    cb: ID3D11Buffer,
    outs: Vec<Output>,
    rt: Option<ID3D11Texture2D>,
    rtv: Option<ID3D11RenderTargetView>,
    stage: Option<ID3D11Texture2D>,
    rt_w: i32,
    rt_h: i32,
    /// Destination rects the render target was last built for; a zoom change
    /// moves them, which means the whole thumbnail is rebuilt even when the
    /// desktop did not change.
    last: Vec<(i32, i32, i32, i32)>,
    /// A CopyResource into the staging texture is waiting to be read. The read
    /// is non-blocking, so it can take two frames before it lands.
    pending: bool,
    /// Event query recorded right after that copy. `Map` on this driver blocks
    /// ~8ms even with DO_NOT_WAIT, so the fence is what keeps the paint thread
    /// moving: only map once the query says the copy has run.
    fence: ID3D11Query,
    /// Set by the `lost` test seam: fail one frame with ACCESS_LOST.
    inject_lost_at: Option<u64>,
    /// `lostloop` seam: fail every frame with ACCESS_LOST, which is what a
    /// duplication that cannot be rebuilt looks like and must end in GDI.
    inject_lost_loop: bool,
    pub acq_us: u64,
    pub draw_us: u64,
    pub read_us: u64,
    pub copy_us: u64,
    pub map_us: u64,
    pub row_us: u64,
    pub frames: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preference {
    Auto,
    Warp,
    Gdi,
    Lost,
    LostLoop,
}

struct State {
    cap: Option<Capture>,
    /// Set once the GPU path is known to be impossible here; the caller then
    /// stays on the GDI fallback instead of retrying every frame.
    disabled: Option<String>,
    last_try: Option<Instant>,
    /// Consecutive ACCESS_LOST rebuilds: three in a row and we stop trying.
    lost: u32,
    /// Unrelated frame errors, same three-strike rule.
    fails: u32,
    pref: Preference,
    logged_pref: bool,
}

impl Default for State {
    fn default() -> State {
        let (pref, raw) = match std::env::var("AI_MOUSE_MINIMAP_GPU").ok() {
            Some(v) => {
                let p = match v.trim().to_ascii_lowercase().as_str() {
                    "warp" => Preference::Warp,
                    "gdi" => Preference::Gdi,
                    "lost" => Preference::Lost,
                    "lostloop" => Preference::LostLoop,
                    _ => Preference::Auto,
                };
                (p, v)
            }
            None => (Preference::Auto, String::new()),
        };
        if !raw.is_empty() {
            crate::logline!("INFO", "minimap: AI_MOUSE_MINIMAP_GPU={raw:?} (test seam)");
        }
        State {
            cap: None,
            disabled: None,
            last_try: None,
            lost: 0,
            fails: 0,
            pref,
            logged_pref: false,
        }
    }
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn hresult(e: &windows::core::Error) -> String {
    format!("{e} (0x{:08X})", e.code().0)
}

fn create_device(
    driver: Driver,
) -> Result<(ID3D11Device, ID3D11DeviceContext, D3D_FEATURE_LEVEL), String> {
    let levels = [
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let mut dev: Option<ID3D11Device> = None;
    let mut ctx: Option<ID3D11DeviceContext> = None;
    let mut level = D3D_FEATURE_LEVEL_10_0;
    let ty = match driver {
        Driver::Hardware => D3D_DRIVER_TYPE_HARDWARE,
        Driver::Warp => D3D_DRIVER_TYPE_WARP,
    };
    unsafe {
        D3D11CreateDevice(
            None,
            ty,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut dev),
            Some(&mut level),
            Some(&mut ctx),
        )
        .map_err(|e| format!("D3D11CreateDevice({}): {}", driver.label(), hresult(&e)))?;
    }
    let dev = dev.ok_or_else(|| "D3D11CreateDevice returned no device".to_string())?;
    let ctx = ctx.ok_or_else(|| "D3D11CreateDevice returned no context".to_string())?;
    Ok((dev, ctx, level))
}

/// The adapter whose outputs get duplicated.
///
/// For a hardware device this must be the device's *own* adapter. For WARP it
/// deliberately is not: WARP drives no display, so asking the real display
/// adapter's output to duplicate through a WARP device is exactly the hybrid
/// graphics failure, and it fails for real with `DXGI_ERROR_UNSUPPORTED`.
fn display_adapter(dev: &ID3D11Device, driver: Driver) -> Result<IDXGIAdapter, String> {
    match driver {
        Driver::Hardware => {
            let dxgi: IDXGIDevice = dev
                .cast()
                .map_err(|e| format!("IDXGIDevice: {}", hresult(&e)))?;
            unsafe { dxgi.GetAdapter() }.map_err(|e| format!("GetAdapter: {}", hresult(&e)))
        }
        Driver::Warp => {
            let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
                .map_err(|e| format!("CreateDXGIFactory1: {}", hresult(&e)))?;
            unsafe { factory.EnumAdapters(0) }
                .map_err(|e| format!("EnumAdapters(0): {}", hresult(&e)))
        }
    }
}

impl Capture {
    fn new(driver: Driver, inject_lost: bool, inject_lost_loop: bool) -> Result<Capture, Failure> {
        let (dev, ctx, level) = create_device(driver).map_err(Failure::Fatal)?;

        let adapter = match display_adapter(&dev, driver) {
            Ok(a) => a,
            Err(e) => return Err(Failure::Unsupported(e)),
        };
        let desc = unsafe { adapter.GetDesc() }
            .map_err(|e| Failure::Fatal(format!("GetAdapter.GetDesc: {}", hresult(&e))))?;
        let adapter_name: String = String::from_utf16_lossy(&desc.Description)
            .trim_end_matches('\0')
            .to_string();

        let mut outs = Vec::new();
        let mut index = 0u32;
        loop {
            let output = match unsafe { adapter.EnumOutputs(index) } {
                Ok(o) => o,
                Err(_) => break, // DXGI_ERROR_NOT_FOUND: no more outputs
            };
            index += 1;
            let odesc = unsafe { output.GetDesc() }
                .map_err(|e| Failure::Fatal(format!("IDXGIOutput.GetDesc: {}", hresult(&e))))?;
            if !odesc.AttachedToDesktop.as_bool() {
                continue;
            }
            let r = odesc.DesktopCoordinates;
            let mon = Monitor {
                name: format!("output{}", index - 1),
                x: r.left,
                y: r.top,
                w: (r.right - r.left).max(1),
                h: (r.bottom - r.top).max(1),
            };
            let output1: IDXGIOutput1 = output
                .cast()
                .map_err(|e| Failure::Fatal(format!("IDXGIOutput1: {}", hresult(&e))))?;
            match unsafe { output1.DuplicateOutput(&dev) } {
                Ok(dupl) => outs.push(Output {
                    dupl,
                    mon,
                    frame: None,
                    srv: None,
                    fw: 0,
                    fh: 0,
                    rotated: odesc.Rotation != DXGI_MODE_ROTATION_IDENTITY,
                }),
                // A device that cannot duplicate this output will not be able to
                // after a retry either: end the chain here.
                Err(e) if cannot_duplicate(e.code()) => {
                    return Err(Failure::Unsupported(format!(
                        "DuplicateOutput({}) on {:?} with {}: {}",
                        mon.name,
                        adapter_name,
                        driver.label(),
                        hresult(&e)
                    )))
                }
                // One monitor refusing must not lose the others.
                Err(e) => crate::logline!(
                    "ERROR",
                    "gpu capture: DuplicateOutput({}) failed: {}",
                    mon.name,
                    hresult(&e)
                ),
            }
        }
        if outs.is_empty() {
            return Err(Failure::Fatal(format!(
                "no monitor accepted Desktop Duplication on {}",
                driver.label()
            )));
        }
        for o in &outs {
            crate::logline!(
                "INFO",
                "gpu capture: {} {}x{} at ({},{}) rotation={}",
                o.mon.name,
                o.mon.w,
                o.mon.h,
                o.mon.x,
                o.mon.y,
                if o.rotated {
                    "non-identity (image will be rotated)"
                } else {
                    "identity"
                }
            );
        }

        let (vs, ps) = compile(&dev).map_err(Failure::Fatal)?;

        let samp = unsafe {
            let mut sd = D3D11_SAMPLER_DESC::default();
            sd.Filter = D3D11_FILTER_MIN_MAG_MIP_LINEAR;
            sd.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
            sd.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
            sd.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
            sd.ComparisonFunc = D3D11_COMPARISON_NEVER;
            sd.MaxAnisotropy = 1;
            sd.MaxLOD = f32::MAX;
            let mut out: Option<ID3D11SamplerState> = None;
            dev.CreateSamplerState(&sd, Some(&mut out))
                .map_err(|e| Failure::Fatal(format!("CreateSamplerState: {}", hresult(&e))))?;
            out.unwrap()
        };

        let cb = unsafe {
            let mut bd = D3D11_BUFFER_DESC::default();
            bd.ByteWidth = std::mem::size_of::<Cb>() as u32;
            // Dynamic: the per-monitor constants are written with Map/Unmap for
            // every draw. Map on a DEFAULT buffer is E_INVALIDARG, which is what
            // the first version of this module got wrong.
            bd.Usage = D3D11_USAGE_DYNAMIC;
            bd.BindFlags = D3D11_BIND_CONSTANT_BUFFER.0 as u32;
            bd.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE.0 as u32;
            let mut out: Option<ID3D11Buffer> = None;
            dev.CreateBuffer(&bd, None, Some(&mut out))
                .map_err(|e| Failure::Fatal(format!("CreateBuffer(cb): {}", hresult(&e))))?;
            out.unwrap()
        };

        let fence = unsafe {
            let qd = D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            };
            let mut out: Option<ID3D11Query> = None;
            dev.CreateQuery(&qd, Some(&mut out))
                .map_err(|e| Failure::Fatal(format!("CreateQuery: {}", hresult(&e))))?;
            out.unwrap()
        };

        crate::logline!(
            "INFO",
            "minimap GPU path: {} on {:?}, feature_level={}, monitors={}",
            driver.label(),
            adapter_name,
            if level == D3D_FEATURE_LEVEL_11_0 {
                "11_0"
            } else if level == D3D_FEATURE_LEVEL_10_1 {
                "10_1"
            } else {
                "10_0"
            },
            outs.len()
        );

        Ok(Capture {
            dev,
            ctx,
            driver,
            vs,
            ps,
            samp,
            cb,
            outs,
            rt: None,
            rtv: None,
            stage: None,
            rt_w: 0,
            rt_h: 0,
            last: Vec::new(),
            pending: false,
            fence,
            inject_lost_at: if inject_lost { Some(20) } else { None },
            inject_lost_loop,
            acq_us: 0,
            draw_us: 0,
            read_us: 0,
            copy_us: 0,
            map_us: 0,
            row_us: 0,
            frames: 0,
        })
    }

    /// Copy an output's duplicated texture into a texture we can sample, making
    /// (and caching) that texture on first use or when the mode changed.
    unsafe fn ensure_frame_tex(&mut self, i: usize, src: &ID3D11Texture2D) -> Result<(), String> {
        let mut sd = D3D11_TEXTURE2D_DESC::default();
        unsafe { src.GetDesc(&mut sd) };
        if self.outs[i].frame.is_some() && self.outs[i].fw == sd.Width && self.outs[i].fh == sd.Height
        {
            return Ok(());
        }
        let mut d = D3D11_TEXTURE2D_DESC::default();
        d.Width = sd.Width;
        d.Height = sd.Height;
        d.MipLevels = 1;
        d.ArraySize = 1;
        d.Format = sd.Format;
        d.SampleDesc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
        d.Usage = D3D11_USAGE_DEFAULT;
        d.BindFlags = D3D11_BIND_SHADER_RESOURCE.0 as u32;
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe { self.dev.CreateTexture2D(&d, None, Some(&mut tex)) }.map_err(|e| {
            format!(
                "CreateTexture2D(frame, {}): {}",
                self.outs[i].mon.name,
                hresult(&e)
            )
        })?;
        let tex = tex.unwrap();
        let mut srv: Option<ID3D11ShaderResourceView> = None;
        unsafe { self.dev.CreateShaderResourceView(&tex, None, Some(&mut srv)) }
            .map_err(|e| format!("CreateShaderResourceView: {}", hresult(&e)))?;
        self.outs[i].fw = sd.Width;
        self.outs[i].fh = sd.Height;
        self.outs[i].srv = srv;
        self.outs[i].frame = Some(tex);
        Ok(())
    }

    /// (Re)build the render target + readback staging texture for a `w`x`h`
    /// thumbnail. The target is cleared once here so a monitor that has not
    /// delivered a frame yet shows the minimap backdrop instead of garbage.
    unsafe fn ensure_target(&mut self, w: i32, h: i32) -> Result<(), String> {
        if self.rt.is_some() && self.rt_w == w && self.rt_h == h {
            return Ok(());
        }
        let mut d = D3D11_TEXTURE2D_DESC::default();
        d.Width = w as u32;
        d.Height = h as u32;
        d.MipLevels = 1;
        d.ArraySize = 1;
        d.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
        d.SampleDesc = DXGI_SAMPLE_DESC { Count: 1, Quality: 0 };
        d.Usage = D3D11_USAGE_DEFAULT;
        d.BindFlags = (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32;
        let mut rt: Option<ID3D11Texture2D> = None;
        unsafe { self.dev.CreateTexture2D(&d, None, Some(&mut rt)) }
            .map_err(|e| format!("CreateTexture2D(rt {w}x{h}): {}", hresult(&e)))?;
        let rt = rt.unwrap();
        let mut rtv: Option<ID3D11RenderTargetView> = None;
        unsafe { self.dev.CreateRenderTargetView(&rt, None, Some(&mut rtv)) }
            .map_err(|e| format!("CreateRenderTargetView: {}", hresult(&e)))?;
        let rtv = rtv.unwrap();

        d.Usage = D3D11_USAGE_STAGING;
        d.BindFlags = 0;
        d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut stage: Option<ID3D11Texture2D> = None;
        unsafe { self.dev.CreateTexture2D(&d, None, Some(&mut stage)) }
            .map_err(|e| format!("CreateTexture2D(staging {w}x{h}): {}", hresult(&e)))?;

        // 0x0E1620, the minimap's own desktop backdrop
        unsafe { self.ctx.ClearRenderTargetView(&rtv, &[0.0549, 0.0863, 0.1255, 1.0]) };
        self.rt = Some(rt);
        self.rtv = Some(rtv);
        self.stage = Some(stage.unwrap());
        self.rt_w = w;
        self.rt_h = h;
        self.last.clear();
        Ok(())
    }

    /// Pull whatever the desktop has delivered, draw it into the thumbnail and
    /// read the (small) result back. Returns true when `thumb` was refreshed.
    fn update(
        &mut self,
        thumb: &mut Thumb,
        w: i32,
        h: i32,
        targets: &[(i32, i32, i32, i32)],
    ) -> Result<bool, String> {
        if self.inject_lost_loop {
            return Err(format!(
                "{ACCESS_LOST}: injected every frame by AI_MOUSE_MINIMAP_GPU=lostloop"
            ));
        }
        if let Some(at) = self.inject_lost_at {
            if self.frames >= at {
                self.inject_lost_at = None;
                return Err(format!(
                    "{ACCESS_LOST}: injected once by AI_MOUSE_MINIMAP_GPU=lost"
                ));
            }
        }
        let t0 = Instant::now();
        let mut need: Vec<usize> = Vec::new();
        for i in 0..self.outs.len() {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            let acquired = unsafe { self.outs[i].dupl.AcquireNextFrame(0, &mut info, &mut res) };
            match acquired {
                Ok(()) => {
                    // LastPresentTime is 0 for mouse-only updates: the image did
                    // not change, so there is nothing to copy or redraw.
                    let changed = info.LastPresentTime != 0 || info.AccumulatedFrames > 0;
                    let mut copied = false;
                    let mut failure: Option<String> = None;
                    if changed || self.outs[i].frame.is_none() {
                        match res.as_ref().map(|r| r.cast::<ID3D11Texture2D>()) {
                            Some(Ok(src)) => match unsafe { self.ensure_frame_tex(i, &src) } {
                                Ok(()) => {
                                    let dst = self.outs[i].frame.clone().unwrap();
                                    unsafe { self.ctx.CopyResource(&dst, &src) };
                                    copied = true;
                                }
                                Err(e) => failure = Some(e),
                            },
                            Some(Err(e)) => {
                                failure = Some(format!("cast duplicated texture: {}", hresult(&e)))
                            }
                            None => {}
                        }
                    }
                    // The frame must be released even when the copy failed,
                    // otherwise the next AcquireNextFrame never succeeds.
                    let _ = unsafe { self.outs[i].dupl.ReleaseFrame() };
                    if let Some(e) = failure {
                        return Err(e);
                    }
                    if copied {
                        need.push(i);
                    }
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {}
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    return Err(format!("{ACCESS_LOST}: {}", hresult(&e)))
                }
                Err(e) => {
                    return Err(format!(
                        "AcquireNextFrame({}): {}",
                        self.outs[i].mon.name,
                        hresult(&e)
                    ))
                }
            }
        }
        self.acq_us += t0.elapsed().as_micros() as u64;

        let geom = self.rt_w != w || self.rt_h != h || self.last.as_slice() != targets;
        if !geom && need.is_empty() && !self.pending {
            return Ok(false);
        }
        let t1 = Instant::now();
        unsafe { self.ensure_target(w, h) }?;

        // Pipeline state is constant for the whole frame; only the per-monitor
        // constants and the source view change between draws.
        unsafe {
            let rtv = self.rtv.clone().unwrap();
            self.ctx.OMSetRenderTargets(Some(&[Some(rtv)]), None);
            let vp = D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            };
            self.ctx.RSSetViewports(Some(&[vp]));
            self.ctx.IASetInputLayout(None::<&ID3D11InputLayout>);
            self.ctx
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.ctx.VSSetShader(&self.vs, None);
            self.ctx.PSSetShader(&self.ps, None);
            self.ctx.PSSetSamplers(0, Some(&[Some(self.samp.clone())]));
            self.ctx.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            self.ctx.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
        }

        // A zoom change moves every destination rect, so the whole thumbnail is
        // rebuilt from the cached monitor frames; otherwise only the monitors
        // that delivered a new frame are redrawn and the rest of the target keeps
        // its previous content.
        let order: Vec<usize> = if geom {
            (0..self.outs.len()).collect()
        } else {
            need
        };
        let (wf, hf) = (w as f32, h as f32);
        for i in order {
            let Some(srv) = self.outs[i].srv.clone() else {
                continue; // this monitor never delivered a frame
            };
            let Some(&(tx, ty, tw, th)) = targets.get(i) else {
                continue;
            };
            if tw <= 0 || th <= 0 {
                continue;
            }
            let cb = Cb {
                dst: [
                    tx as f32 / wf * 2.0 - 1.0,
                    1.0 - ty as f32 / hf * 2.0,
                    (tx + tw) as f32 / wf * 2.0 - 1.0,
                    1.0 - (ty + th) as f32 / hf * 2.0,
                ],
                uv: [0.0, 0.0, 1.0, 1.0],
                foot: [1.0 / tw as f32, 1.0 / th as f32],
                _pad: [0.0, 0.0],
            };
            unsafe {
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                self.ctx
                    .Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
                    .map_err(|e| format!("Map(cb): {}", hresult(&e)))?;
                std::ptr::copy_nonoverlapping(
                    &cb as *const Cb as *const u8,
                    mapped.pData as *mut u8,
                    std::mem::size_of::<Cb>(),
                );
                self.ctx.Unmap(&self.cb, 0);
                self.ctx.PSSetShaderResources(0, Some(&[Some(srv)]));
                self.ctx.Draw(3, 0);
            }
        }
        // Unbind the source views before the staging copy so no resource is left
        // bound while it is read.
        unsafe { self.ctx.PSSetShaderResources(0, Some(&[None])) };
        self.draw_us += t1.elapsed().as_micros() as u64;

        let t2 = Instant::now();
        let budget = (w.max(1) * h.max(1) * 4) as usize;
        if thumb.px.len() != budget {
            thumb.px = vec![0u8; budget];
        }
        let staged = {
            let (Some(rt), Some(stage)) = (self.rt.as_ref(), self.stage.as_ref()) else {
                return Err("render target missing".to_string());
            };
            let tc = Instant::now();
            if !self.pending {
                // The rendered frame is copied once; reading it back is what may
                // have to wait for the GPU, so the copy is queued here and picked
                // up by whichever call finds it finished.
                unsafe { self.ctx.CopyResource(stage, rt) };
                // fence the copy: Map blocks on this driver until the GPU has
                // resolved the staging texture, and a minimap may not stall the
                // paint loop for milliseconds every frame
                unsafe { self.ctx.End(&self.fence) };
                self.pending = true;
            }
            self.copy_us += tc.elapsed().as_micros() as u64;
            // has the fence signalled? GetData never blocks; a 0 means "not yet".
            let mut done: u32 = 0;
            let _ = unsafe {
                self.ctx.GetData(
                    &self.fence,
                    Some(&mut done as *mut u32 as *mut c_void),
                    4,
                    0,
                )
            };
            let tm = Instant::now();
            if done == 0 {
                self.map_us += tm.elapsed().as_micros() as u64;
                return Ok(false); // keep the previous thumbnail, retry next frame
            }
            // The fence says the copy has run, so this should not block; the flag
            // is a safety net for drivers that resolve lazily anyway.
            let flag = D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            let out = match unsafe { self.ctx.Map(stage, 0, D3D11_MAP_READ, flag, Some(&mut mapped)) }
            {
                Ok(()) => Some(mapped),
                Err(e) if e.code() == DXGI_ERROR_WAS_STILL_DRAWING => None,
                Err(e) => return Err(format!("Map(staging): {}", hresult(&e))),
            };
            self.map_us += tm.elapsed().as_micros() as u64;
            out
        };
        let mapped = match staged {
            Some(m) => m,
            // Still on the GPU: keep the previous thumbnail, retry next frame.
            None => {
                self.read_us += t2.elapsed().as_micros() as u64;
                return Ok(false);
            }
        };
        let tr = Instant::now();
        {
            let row = (w.max(1) * 4) as usize;
            for y in 0..h.max(1) as usize {
                let src = unsafe { (mapped.pData as *const u8).add(y * mapped.RowPitch as usize) };
                unsafe {
                    std::ptr::copy_nonoverlapping(src, thumb.px[y * row..].as_mut_ptr(), row);
                }
            }
            if let Some(stage) = self.stage.as_ref() {
                unsafe { self.ctx.Unmap(stage, 0) };
            }
        }
        self.row_us += tr.elapsed().as_micros() as u64;
        self.read_us += t2.elapsed().as_micros() as u64;
        self.pending = false;
        self.frames += 1;
        self.last.clear();
        self.last.extend_from_slice(targets);
        thumb.w = w;
        thumb.h = h;
        thumb.valid = true;
        Ok(true)
    }
}

fn compile(dev: &ID3D11Device) -> Result<(ID3D11VertexShader, ID3D11PixelShader), String> {
    let src = HLSL.as_bytes();
    // One source, two entry points and two shader models (vs_4_0 / ps_4_0 keeps
    // it inside Direct3D 10 class hardware, which feature level 10_0 guarantees).
    let vs_code = compile_one(src, "VSMain", "vs_4_0")?;
    let ps_code = compile_one(src, "PSMain", "ps_4_0")?;
    let mut vs: Option<ID3D11VertexShader> = None;
    let mut ps: Option<ID3D11PixelShader> = None;
    unsafe {
        dev.CreateVertexShader(&vs_code, None, Some(&mut vs))
            .map_err(|e| format!("CreateVertexShader: {}", hresult(&e)))?;
        dev.CreatePixelShader(&ps_code, None, Some(&mut ps))
            .map_err(|e| format!("CreatePixelShader: {}", hresult(&e)))?;
    }
    Ok((
        vs.ok_or_else(|| "no vertex shader".to_string())?,
        ps.ok_or_else(|| "no pixel shader".to_string())?,
    ))
}

fn compile_one(src: &[u8], entry: &str, target: &str) -> Result<Vec<u8>, String> {
    use std::ffi::CString;
    // D3DCompile takes ANSI entry point / profile names
    let entry_c = CString::new(entry).map_err(|e| format!("entry {entry}: {e}"))?;
    let target_c = CString::new(target).map_err(|e| format!("target {target}: {e}"))?;
    let entry_p = PCSTR::from_raw(entry_c.as_ptr() as *const u8);
    let target_p = PCSTR::from_raw(target_c.as_ptr() as *const u8);
    let mut blob: Option<windows::Win32::Graphics::Direct3D::ID3DBlob> = None;
    let mut errs: Option<windows::Win32::Graphics::Direct3D::ID3DBlob> = None;
    let rc = unsafe {
        D3DCompile(
            src.as_ptr() as *const c_void,
            src.len(),
            PCSTR::null(),
            None,
            None,
            entry_p,
            target_p,
            0,
            0,
            &mut blob,
            Some(&mut errs),
        )
    };
    if let Err(e) = rc {
        let msg = errs
            .as_ref()
            .map(|b| unsafe {
                let p = b.GetBufferPointer() as *const u8;
                let n = b.GetBufferSize();
                String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).to_string()
            })
            .unwrap_or_default();
        return Err(format!(
            "D3DCompile({entry} {target}): {} {msg}",
            hresult(&e)
        ));
    }
    let b = blob.ok_or_else(|| "D3DCompile returned no bytecode".to_string())?;
    Ok(unsafe {
        std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec()
    })
}

/// Walk the chain: hardware, then WARP, stopping at the first device that can
/// actually duplicate a monitor. `Unsupported` ends the walk immediately - see
/// the module docs.
fn open(pref: Preference) -> Result<Capture, Failure> {
    let inject_lost = pref == Preference::Lost;
    let inject_lost_loop = pref == Preference::LostLoop;
    let order: &[Driver] = match pref {
        Preference::Warp => &[Driver::Warp],
        _ => &[Driver::Hardware, Driver::Warp],
    };
    let mut last = Failure::Fatal("no driver was attempted".to_string());
    for (i, driver) in order.iter().enumerate() {
        let more = i + 1 < order.len();
        match Capture::new(*driver, inject_lost, inject_lost_loop) {
            Ok(cap) => return Ok(cap),
            Err(Failure::Unsupported(why)) => {
                crate::logline!(
                    "ERROR",
                    "minimap GPU path unsupported: {why} -> falling back to GDI capture"
                );
                return Err(Failure::Unsupported(why));
            }
            Err(Failure::Fatal(why)) => {
                crate::logline!(
                    "WARN",
                    "minimap GPU path: {} unusable ({why}) -> {}",
                    driver.label(),
                    if more {
                        "trying WARP"
                    } else {
                        "falling back to GDI capture"
                    }
                );
                last = Failure::Fatal(why);
            }
        }
    }
    Err(last)
}

/// Render the live desktop into a `w`x`h` thumbnail on the GPU.
///
/// `target` maps each monitor's virtual-desktop rect onto a rect in the
/// thumbnail, which is where multi-monitor layout comes from: the caller decides
/// the scale (minimap size over desktop size) and each monitor lands where it
/// physically sits.
///
/// `Ok(true)` means `thumb` was refreshed, `Ok(false)` means the desktop has not
/// delivered a new frame (or the GPU path is being rebuilt), and `Err` means the
/// GPU path is not usable at all - the caller must use the GDI capture, which is
/// exactly what it does on the first `Err` and on every call afterwards.
pub fn desktop_thumb(
    thumb: &mut Thumb,
    w: i32,
    h: i32,
    target: impl Fn(&Monitor) -> (i32, i32, i32, i32),
) -> Result<bool, String> {
    if w <= 0 || h <= 0 {
        return Ok(false);
    }
    let state = STATE.get_or_init(|| Mutex::new(State::default()));
    let mut g = state
        .lock()
        .map_err(|_| "gpu capture state poisoned".to_string())?;
    if let Some(reason) = &g.disabled {
        return Err(reason.clone());
    }
    if !g.logged_pref {
        g.logged_pref = true;
        if g.pref == Preference::Gdi {
            let why = "AI_MOUSE_MINIMAP_GPU=gdi: GPU capture skipped on request".to_string();
            crate::logline!("ERROR", "minimap GPU path skipped: {why} -> GDI capture");
            g.disabled = Some(why.clone());
            return Err(why);
        }
    }
    if g.cap.is_none() {
        // A lost duplication is rebuilt, but not on every single frame.
        if let Some(t) = g.last_try {
            if t.elapsed() < Duration::from_millis(800) {
                return Ok(false);
            }
        }
        g.last_try = Some(Instant::now());
        match open(g.pref) {
            Ok(c) => {
                g.cap = Some(c);
                return Ok(false); // the first real frame arrives on the next call
            }
            Err(Failure::Unsupported(why)) => {
                crate::logline!("ERROR", "minimap: GPU capture disabled for good: {why}");
                g.disabled = Some(why.clone());
                return Err(why);
            }
            Err(Failure::Fatal(why)) => {
                crate::logline!("ERROR", "minimap: GPU capture disabled for good: {why}");
                g.disabled = Some(why.clone());
                return Err(why);
            }
        }
    }
    let cap = g.cap.as_mut().unwrap();
    let targets: Vec<(i32, i32, i32, i32)> = cap.outs.iter().map(|o| target(&o.mon)).collect();
    match cap.update(thumb, w, h, &targets) {
        Ok(changed) => {
            if changed {
                g.lost = 0;
                g.fails = 0;
            }
            Ok(changed)
        }
        Err(e) if e.starts_with(ACCESS_LOST) => {
            g.lost += 1;
            if g.lost > 3 {
                let why = format!("{e} (lost {} times in a row)", g.lost);
                crate::logline!(
                    "ERROR",
                    "minimap GPU path: dropping to GDI capture after repeated losses: {why}"
                );
                g.cap = None;
                g.disabled = Some(why.clone());
                return Err(why);
            }
            // Recoverable: drop the capture and rebuild it, keeping the last
            // thumbnail on screen in the meantime.
            crate::logline!(
                "WARN",
                "minimap GPU path: {e} -> rebuilding duplication (attempt {}/3)",
                g.lost
            );
            g.cap = None;
            Ok(false)
        }
        Err(e) => {
            crate::logline!("ERROR", "minimap GPU path: frame failed: {e}");
            g.cap = None;
            g.fails += 1;
            if g.fails >= 3 {
                crate::logline!(
                    "ERROR",
                    "minimap GPU path: {} frame errors in a row -> GDI capture",
                    g.fails
                );
                g.disabled = Some(e);
                return Err("repeated frame errors".to_string());
            }
            Ok(false)
        }
    }
}

/// What the capture has done so far, for the minimap's own stats line.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub frames: u64,
    pub acq_us: u64,
    pub draw_us: u64,
    pub read_us: u64,
    pub copy_us: u64,
    pub map_us: u64,
    pub row_us: u64,
}

pub fn stats() -> Option<Stats> {
    let g = STATE.get()?.lock().ok()?;
    let c = g.cap.as_ref()?;
    Some(Stats {
        frames: c.frames,
        acq_us: c.acq_us,
        draw_us: c.draw_us,
        read_us: c.read_us,
        copy_us: c.copy_us,
        map_us: c.map_us,
        row_us: c.row_us,
    })
}
