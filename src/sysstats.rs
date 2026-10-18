//! CPU / RAM / GPU readings.
use std::collections::HashMap;
use std::ffi::c_void;
use std::io::BufRead;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use crate::*;


// CPU / RAM / GPU readings, refreshed once a second by a background thread.
pub(crate) struct SysStats {
    pub(crate) cpu: AtomicU32,
    pub(crate) ram: AtomicU32,
    pub(crate) gpu: AtomicU32,
    // The GPU counters are only queried while the GPU item is enabled.
    pub(crate) want_gpu: AtomicBool,
}

impl SysStats {
    pub(crate) fn new() -> Self {
        Self {
            cpu: AtomicU32::new(STAT_NONE),
            ram: AtomicU32::new(STAT_NONE),
            gpu: AtomicU32::new(STAT_NONE),
            want_gpu: AtomicBool::new(false),
        }
    }
}

pub(crate) static SYS: OnceLock<Arc<SysStats>> = OnceLock::new();

pub(crate) fn filetime_u64(ft: &FILETIME) -> u64 {
    ((ft.high as u64) << 32) | ft.low as u64
}

pub(crate) fn wide_ptr_to_string(ptr: *const u16) -> String {
    let mut units: Vec<u16> = Vec::new();
    if ptr.is_null() { return String::new(); }
    unsafe {
        let mut i = 0usize;
        while i < 512 {
            let unit = *ptr.add(i);
            if unit == 0 { break; }
            units.push(unit);
            i += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

// Reads GPU utilization from the Windows "GPU Engine" performance counters (the same
// source Task Manager uses). It works for NVIDIA, AMD and Intel GPUs on Windows 10+.
pub(crate) struct GpuSampler {
    pub(crate) query: HANDLE,
    pub(crate) counter: HANDLE,
}

impl GpuSampler {
    pub(crate) fn new() -> Result<Self, i32> {
        unsafe {
            let mut query: HANDLE = std::ptr::null_mut();
            let status = PdhOpenQueryW(std::ptr::null(), 0, &mut query);
            if status != 0 { return Err(status); }
            let path = wide("\\GPU Engine(*)\\Utilization Percentage");
            let mut counter: HANDLE = std::ptr::null_mut();
            let status = PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter);
            if status != 0 {
                PdhCloseQuery(query);
                return Err(status);
            }
            // The first collection only establishes a baseline.
            let _ = PdhCollectQueryData(query);
            Ok(Self { query, counter })
        }
    }

    // Busiest engine (3D, compute, copy, video...) of the busiest GPU, in percent.
    pub(crate) fn sample(&mut self) -> Option<u32> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 { return None; }
            let mut size: DWORD = 0;
            let mut count: DWORD = 0;
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, std::ptr::null_mut());
            if status as u32 != PDH_MORE_DATA || size == 0 { return None; }
            // u64 elements keep the buffer 8-byte aligned for PdhCounterValueItem.
            let mut buffer: Vec<u64> = vec![0; (size as usize + 7) / 8 + 1];
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, buffer.as_mut_ptr() as *mut c_void);
            if status != 0 { return None; }

            let items = buffer.as_ptr() as *const PdhCounterValueItem;
            let mut totals: HashMap<(String, String), f64> = HashMap::new();
            for i in 0..count as usize {
                let item = &*items.add(i);
                // PDH_CSTATUS_VALID_DATA = 0, PDH_CSTATUS_NEW_DATA = 1
                if item.status != 0 && item.status != 1 { continue; }
                let name = wide_ptr_to_string(item.name);
                let luid = name.split("luid_").nth(1)
                    .and_then(|rest| rest.split("_phys").next())
                    .unwrap_or("")
                    .to_string();
                let engine = name.rsplit("engtype_").next().unwrap_or("").to_string();
                *totals.entry((luid, engine)).or_insert(0.0) += item.double_value;
            }
            let busiest = totals.values().copied().fold(0.0_f64, f64::max);
            Some(busiest.round().clamp(0.0, 100.0) as u32)
        }
    }
}

impl Drop for GpuSampler {
    fn drop(&mut self) {
        unsafe { PdhCloseQuery(self.query); }
    }
}

pub(crate) fn spawn_system_stats_thread(stats: Arc<SysStats>, log_path: PathBuf) {
    std::thread::spawn(move || {
        let mut previous_cpu: Option<(u64, u64)> = None; // (idle, total) system times
        let mut gpu_sampler: Option<GpuSampler> = None;
        let mut gpu_error_logged = false;
        let mut gpu_samples_taken = 0u32;
        let mut gpu_misses = 0u32;

        loop {
            unsafe {
                let mut idle = FILETIME::default();
                let mut kernel = FILETIME::default();
                let mut user = FILETIME::default();
                if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
                    let idle_now = filetime_u64(&idle);
                    // Kernel time already includes idle time.
                    let total_now = filetime_u64(&kernel) + filetime_u64(&user);
                    if let Some((idle_before, total_before)) = previous_cpu {
                        let total_delta = total_now.saturating_sub(total_before);
                        let idle_delta = idle_now.saturating_sub(idle_before).min(total_delta);
                        if total_delta > 0 {
                            let busy = (total_delta - idle_delta) as f64 / total_delta as f64 * 100.0;
                            stats.cpu.store(busy.round().clamp(0.0, 100.0) as u32, Ordering::Relaxed);
                        }
                    }
                    previous_cpu = Some((idle_now, total_now));
                }

                let mut memory: MEMORYSTATUSEX = zeroed();
                memory.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
                if GlobalMemoryStatusEx(&mut memory) != 0 {
                    stats.ram.store(memory.dwMemoryLoad.min(100), Ordering::Relaxed);
                }
            }

            if stats.want_gpu.load(Ordering::Relaxed) {
                let mut just_created = false;
                if gpu_sampler.is_none() {
                    match GpuSampler::new() {
                        Ok(sampler) => {
                            gpu_sampler = Some(sampler);
                            gpu_samples_taken = 0;
                            just_created = true;
                        }
                        Err(status) => {
                            if !gpu_error_logged {
                                log_event(&log_path, &format!("GPU usage counters unavailable (PDH status 0x{:08X})", status as u32));
                                gpu_error_logged = true;
                            }
                            stats.gpu.store(STAT_NONE, Ordering::Relaxed);
                        }
                    }
                }
                // A sample needs a baseline from an earlier collection, so a new query waits one tick.
                if !just_created {
                    if let Some(sampler) = gpu_sampler.as_mut() {
                        match sampler.sample() {
                            Some(value) => {
                                stats.gpu.store(value, Ordering::Relaxed);
                                gpu_misses = 0;
                            }
                            None => {
                                gpu_misses += 1;
                                if gpu_misses >= 4 { stats.gpu.store(STAT_NONE, Ordering::Relaxed); }
                            }
                        }
                        gpu_samples_taken += 1;
                    }
                }
                // Wildcard counters are expanded when they are added, so rebuild the query now
                // and then to pick up the GPU engines of programs started in the meantime.
                if gpu_samples_taken >= 30 { gpu_sampler = None; }
            } else if gpu_sampler.is_some() {
                gpu_sampler = None;
                stats.gpu.store(STAT_NONE, Ordering::Relaxed);
            }

            std::thread::sleep(Duration::from_millis(1000));
        }
    });
}

