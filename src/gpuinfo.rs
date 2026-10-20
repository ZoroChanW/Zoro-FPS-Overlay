//! GPU memory use and temperature.
//!
//! NVIDIA: read from NVML, the library that ships with the driver (nvml.dll). No extra install.
//! Other GPUs: memory use from the Windows "GPU Adapter Memory" counters, total memory from
//! DXGI, and temperature from the same Windows interface Task Manager uses (D3DKMT).
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use crate::*;

// The GPU with the most dedicated memory, which is the one games run on.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuAdapter {
    pub(crate) luid_low: u32,
    pub(crate) luid_high: i32,
    pub(crate) dedicated_mb: u32,
}

unsafe fn vtable_entry(object: *mut c_void, index: usize) -> *const c_void {
    let table = *(object as *const *const *const c_void);
    *table.add(index)
}

unsafe fn release(object: *mut c_void) {
    let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vtable_entry(object, 2));
    release(object);
}

pub(crate) fn primary_gpu() -> Option<GpuAdapter> {
    unsafe {
        // IID_IDXGIFactory1 {770aae78-f26f-4dba-a829-253c83d1b387}
        let iid = GUID { d1: 0x770a_ae78, d2: 0xf26f, d3: 0x4dba, d4: [0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87] };
        let mut factory: *mut c_void = std::ptr::null_mut();
        if CreateDXGIFactory1(&iid, &mut factory) < 0 || factory.is_null() { return None; }
        // IDXGIFactory1::EnumAdapters1 is entry 12 of the vtable; IDXGIAdapter1::GetDesc1 is entry 10.
        let enum_adapters: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 =
            std::mem::transmute(vtable_entry(factory, 12));
        let mut best: Option<GpuAdapter> = None;
        for index in 0..16u32 {
            let mut adapter: *mut c_void = std::ptr::null_mut();
            if enum_adapters(factory, index, &mut adapter) < 0 || adapter.is_null() { break; }
            let get_desc: unsafe extern "system" fn(*mut c_void, *mut DXGI_ADAPTER_DESC1) -> i32 =
                std::mem::transmute(vtable_entry(adapter, 10));
            let mut desc: DXGI_ADAPTER_DESC1 = zeroed();
            if get_desc(adapter, &mut desc) >= 0 && desc.flags & DXGI_ADAPTER_FLAG_SOFTWARE == 0 {
                let dedicated_mb = (desc.dedicated_video_memory / (1024 * 1024)).min(u32::MAX as usize) as u32;
                if best.map_or(true, |b| dedicated_mb > b.dedicated_mb) {
                    best = Some(GpuAdapter { luid_low: desc.luid_low, luid_high: desc.luid_high, dedicated_mb });
                }
            }
            release(adapter);
        }
        release(factory);
        best
    }
}

// ------------------------------------------------------------------ NVML (NVIDIA)

#[repr(C)]
struct NvmlMemory { total: u64, free: u64, used: u64 }

pub(crate) struct Nvml {
    device: *mut c_void,
    get_temperature: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32,
    get_memory: unsafe extern "C" fn(*mut c_void, *mut NvmlMemory) -> i32,
}

// The handle is only used from the sampling thread.
unsafe impl Send for Nvml {}

impl Nvml {
    pub(crate) fn load() -> Option<Nvml> {
        unsafe {
            let mut library = LoadLibraryW(wide("nvml.dll").as_ptr());
            if library.is_null() {
                library = LoadLibraryW(wide("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll").as_ptr());
            }
            if library.is_null() { return None; }
            let symbol = |name: &[u8]| GetProcAddress(library, name.as_ptr());
            let (init, count, by_index, temperature, memory) = (
                symbol(b"nvmlInit_v2\0"), symbol(b"nvmlDeviceGetCount_v2\0"), symbol(b"nvmlDeviceGetHandleByIndex_v2\0"),
                symbol(b"nvmlDeviceGetTemperature\0"), symbol(b"nvmlDeviceGetMemoryInfo\0"),
            );
            if init.is_null() || count.is_null() || by_index.is_null() || temperature.is_null() || memory.is_null() { return None; }
            let init: unsafe extern "C" fn() -> i32 = std::mem::transmute(init);
            let count: unsafe extern "C" fn(*mut u32) -> i32 = std::mem::transmute(count);
            let by_index: unsafe extern "C" fn(u32, *mut *mut c_void) -> i32 = std::mem::transmute(by_index);
            let get_temperature: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32 = std::mem::transmute(temperature);
            let get_memory: unsafe extern "C" fn(*mut c_void, *mut NvmlMemory) -> i32 = std::mem::transmute(memory);
            if init() != 0 { return None; }
            let mut devices = 0u32;
            if count(&mut devices) != 0 || devices == 0 { return None; }
            // With several NVIDIA GPUs, use the one with the most memory.
            let mut best: Option<(*mut c_void, u64)> = None;
            for index in 0..devices.min(8) {
                let mut device: *mut c_void = std::ptr::null_mut();
                if by_index(index, &mut device) != 0 || device.is_null() { continue; }
                let mut info: NvmlMemory = zeroed();
                if get_memory(device, &mut info) != 0 { continue; }
                if best.map_or(true, |(_, total)| info.total > total) { best = Some((device, info.total)); }
            }
            let (device, _) = best?;
            Some(Nvml { device, get_temperature, get_memory })
        }
    }

    // (used MB, total MB)
    pub(crate) fn memory_mb(&self) -> Option<(u32, u32)> {
        unsafe {
            let mut info: NvmlMemory = zeroed();
            if (self.get_memory)(self.device, &mut info) != 0 { return None; }
            Some(((info.used / (1024 * 1024)) as u32, (info.total / (1024 * 1024)) as u32))
        }
    }

    pub(crate) fn temperature_c(&self) -> Option<u32> {
        let mut value = 0u32;
        // NVML_TEMPERATURE_GPU = 0
        let status = unsafe { (self.get_temperature)(self.device, 0, &mut value) };
        if status == 0 && (1..=150).contains(&value) { Some(value) } else { None }
    }
}

// ------------------------------------------------------------------ Windows (any GPU)

// GPU temperature as Windows reports it for the adapter (tenths of a degree).
pub(crate) fn windows_temperature_c(adapter: &GpuAdapter) -> Option<u32> {
    unsafe {
        let mut open = D3DKMT_OPENADAPTERFROMLUID { luid_low: adapter.luid_low, luid_high: adapter.luid_high, handle: 0 };
        if D3DKMTOpenAdapterFromLuid(&mut open) != 0 || open.handle == 0 { return None; }
        let mut data: D3DKMT_ADAPTER_PERFDATA = zeroed();
        let query = D3DKMT_QUERYADAPTERINFO {
            handle: open.handle,
            kind: KMTQAITYPE_ADAPTERPERFDATA,
            data: &mut data as *mut D3DKMT_ADAPTER_PERFDATA as *mut c_void,
            size: size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
        };
        let status = D3DKMTQueryAdapterInfo(&query);
        let close = D3DKMT_CLOSEADAPTER { handle: open.handle };
        D3DKMTCloseAdapter(&close);
        if status != 0 { return None; }
        let degrees = data.temperature / 10;
        if (1..=150).contains(&degrees) { Some(degrees) } else { None }
    }
}

// Dedicated GPU memory in use, from the "GPU Adapter Memory" performance counters.
pub(crate) struct VramCounter {
    query: HANDLE,
    counter: HANDLE,
}

unsafe impl Send for VramCounter {}

impl VramCounter {
    pub(crate) fn new() -> Option<VramCounter> {
        unsafe {
            let mut query: HANDLE = std::ptr::null_mut();
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 { return None; }
            let mut counter: HANDLE = std::ptr::null_mut();
            let path = wide("\\GPU Adapter Memory(*)\\Dedicated Usage");
            if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                PdhCloseQuery(query);
                return None;
            }
            let _ = PdhCollectQueryData(query);
            Some(VramCounter { query, counter })
        }
    }

    // Used MB for the given adapter.
    pub(crate) fn used_mb(&mut self, adapter: &GpuAdapter) -> Option<u32> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 { return None; }
            let mut size: DWORD = 0;
            let mut count: DWORD = 0;
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut());
            if status as u32 != PDH_MORE_DATA || size == 0 { return None; }
            let mut buffer: Vec<u64> = vec![0; (size as usize + 7) / 8 + 1];
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, buffer.as_mut_ptr() as *mut c_void);
            if status != 0 { return None; }
            let wanted = format!("luid_0x{:08X}_0x{:08X}", adapter.luid_high as u32, adapter.luid_low);
            let items = buffer.as_ptr() as *const PdhCounterValueItem;
            let mut matched: Option<f64> = None;
            let mut largest: f64 = 0.0;
            for i in 0..count as usize {
                let item = &*items.add(i);
                if item.status != 0 && item.status != 1 { continue; }
                let name = wide_ptr_to_string(item.name);
                largest = largest.max(item.double_value);
                if name.to_ascii_lowercase().contains(&wanted.to_ascii_lowercase()) {
                    matched = Some(matched.unwrap_or(0.0) + item.double_value);
                }
            }
            // If no instance carries this adapter's id, fall back to the largest one.
            let bytes = matched.unwrap_or(largest);
            Some((bytes / (1024.0 * 1024.0)).round().clamp(0.0, u32::MAX as f64 - 1.0) as u32)
        }
    }
}

impl Drop for VramCounter {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query); }
    }
}

// Everything needed to read VRAM and temperature, created when one of them is switched on.
pub(crate) struct GpuInfo {
    adapter: Option<GpuAdapter>,
    nvml: Option<Nvml>,
    vram_counter: Option<VramCounter>,
    pub(crate) vram_source: &'static str,
    pub(crate) temperature_source: &'static str,
}

impl GpuInfo {
    pub(crate) fn new(log_path: &PathBuf) -> GpuInfo {
        let adapter = primary_gpu();
        let nvml = Nvml::load();
        let info = GpuInfo {
            vram_counter: if nvml.is_none() { VramCounter::new() } else { None },
            vram_source: if nvml.is_some() { "NVML" } else { "Windows counters + DXGI" },
            temperature_source: if nvml.is_some() { "NVML" } else { "Windows adapter data" },
            adapter,
            nvml,
        };
        log_event(log_path, &format!(
            "GPU info: adapter={:?} vram source={} temperature source={}",
            info.adapter, info.vram_source, info.temperature_source
        ));
        info
    }

    // (used MB, total MB)
    pub(crate) fn vram_mb(&mut self) -> Option<(u32, u32)> {
        if let Some(nvml) = &self.nvml {
            return nvml.memory_mb();
        }
        let adapter = self.adapter?;
        let used = self.vram_counter.as_mut()?.used_mb(&adapter)?;
        Some((used, adapter.dedicated_mb))
    }

    pub(crate) fn temperature_c(&mut self) -> Option<u32> {
        if let Some(nvml) = &self.nvml {
            if let Some(value) = nvml.temperature_c() { return Some(value); }
        }
        windows_temperature_c(&self.adapter?)
    }
}
