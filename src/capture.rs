//! PresentMon capture: process control and CSV parsing.
use std::os::windows::process::CommandExt;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;
use std::sync::{Mutex, OnceLock};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::mem::{size_of, zeroed};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::*;

// Values produced by the PresentMon reader thread and read by the overlay timer.
#[derive(Default)]
pub(crate) struct SharedStats {
    pub(crate) fps: AtomicU32,
    // Average frametime in hundredths of a millisecond (6.94 ms -> 694).
    pub(crate) frametime_x100: AtomicU32,
    pub(crate) low1_fps: AtomicU32,
    pub(crate) last_sample_ms: AtomicU64,
    // Recent frame times in milliseconds (newest last), for the optional graph.
    pub(crate) frames: Mutex<VecDeque<f32>>,
}

pub(crate) struct Capture {
    pub(crate) child: Child,
    pub(crate) process_name: String,
    pub(crate) process_id: u32,
    pub(crate) session_name: String,
    pub(crate) stats: Arc<SharedStats>,
}


pub(crate) fn cleanup_stale_session_from_previous_run(presentmon_path: &PathBuf, diagnostic_path: &PathBuf, current_pid: u32) {
    if !presentmon_path.exists() { return; }
    let Ok(previous_log) = fs::read_to_string(diagnostic_path) else { return; };
    let Some(session_name) = previous_log.lines().rev().find_map(|line| {
        line.split_once("Launching PresentMon with session=")
            .map(|(_, value)| value.trim().to_string())
    }) else { return; };
    let Some(owner_pid) = session_name.strip_prefix("ZoroFPS").and_then(|s| s.parse::<u32>().ok()) else { return; };
    if owner_pid == current_pid || process_id_is_running(owner_pid) { return; }

    let result = Command::new(presentmon_path)
        .arg("--terminate_existing_session")
        .arg("--session_name")
        .arg(&session_name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if result.is_err() {
        // Startup diagnostics are reset below; this is intentionally best-effort.
        // The new capture also has --stop_existing_session as a recovery path.
    }
}

pub(crate) fn stop_capture(state: &mut AppState) {
    if let Some(mut capture) = state.capture.take() {
        log_event(
            &state.diagnostic_path,
            &format!("Stopping PresentMon: target={} pid={} session={}", capture.process_name, capture.process_id, capture.session_name),
        );

        // Do not kill PresentMon first: abruptly terminating it can leave its ETW
        // trace session registered. Ask the official CLI to stop that session,
        // then give the collector a brief chance to exit normally.
        if state.presentmon_path.exists() {
            match Command::new(&state.presentmon_path)
                .arg("--terminate_existing_session")
                .arg("--session_name")
                .arg(&capture.session_name)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status()
            {
                Ok(status) => log_event(
                    &state.diagnostic_path,
                    &format!("PresentMon session-stop helper exited with {}", status),
                ),
                Err(e) => log_event(
                    &state.diagnostic_path,
                    &format!("WARNING: could not run PresentMon session-stop helper: {}", e),
                ),
            }
        }

        let deadline = Instant::now() + Duration::from_millis(350);
        loop {
            match capture.child.try_wait() {
                Ok(Some(status)) => {
                    log_event(&state.diagnostic_path, &format!("PresentMon collector exited normally with {}", status));
                    break;
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                Ok(None) => {
                    log_event(&state.diagnostic_path, "WARNING: PresentMon collector did not exit after session stop; terminating it");
                    let _ = capture.child.kill();
                    let _ = capture.child.wait();
                    break;
                }
                Err(e) => {
                    log_event(&state.diagnostic_path, &format!("WARNING: failed waiting for PresentMon collector: {}", e));
                    let _ = capture.child.kill();
                    let _ = capture.child.wait();
                    break;
                }
            }
        }
    }
    state.target_name = None;
    state.target_pid = None;
    state.clear_capture_values("");
}

// PresentMon children are placed in a job that Windows closes together with this process,
// so a crash or a kill of the overlay never leaves PresentMon running in the background.
static JOB: OnceLock<usize> = OnceLock::new();

fn kill_on_close_job() -> HANDLE {
    *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() { return 0; }
        let mut limits: JobExtendedLimits = zeroed();
        limits.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            &limits as *const JobExtendedLimits as *const c_void,
            size_of::<JobExtendedLimits>() as DWORD,
        );
        if ok == 0 { CloseHandle(job); return 0; }
        job as usize
    }) as HANDLE
}

fn attach_to_job(child: &Child, log_path: &PathBuf) {
    let job = kill_on_close_job();
    if job.is_null() {
        log_event(log_path, "WARNING: could not create the PresentMon job object");
        return;
    }
    let assigned = unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) };
    if assigned == 0 {
        log_event(log_path, "WARNING: could not add PresentMon to the job object");
    }
}

pub(crate) fn start_capture(state: &mut AppState, process_name: &str, process_id: u32) {
    stop_capture(state);
    state.target_name = Some(process_name.to_string());
    state.target_pid = Some(process_id);
    state.rate_since = None;
    state.confirmed = false;
    state.last_flowing = None;
    state.recorded_current = false;
    state.capture_last_focus = Instant::now();

    log_event(&state.diagnostic_path, &format!(
        "Starting capture: process={} pid={} presentmon={}",
        process_name, process_id, state.presentmon_path.display()
    ));
    if !state.presentmon_path.exists() {
        log_event(&state.diagnostic_path, "ERROR: PresentMon.exe not found next to overlay executable");
        state.clear_capture_values("ERR");
        return;
    }

    let session_name = format!("ZoroFPS{}", state.own_pid);
    // Keep PresentMon diagnostics on disk, but read CSV from stdout. PresentMon
    // opens --output_file in a way that prevents this process from reading the
    // live CSV on some Windows versions (sharing violation / OS error 32).
    let log_stderr = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.presentmon_output_path)
    {
        Ok(file) => file,
        Err(e) => {
            state.clear_capture_values("ERR");
            log_event(&state.diagnostic_path, &format!("ERROR: could not open PresentMon error log: {}", e));
            return;
        }
    };

    let mut command = Command::new(&state.presentmon_path);
    command
        .arg("--process_id").arg(process_id.to_string())
        .arg("--output_stdout")
        .arg("--no_console_stats")
        .arg("--v1_metrics")
        // These options reduce work unrelated to a simple FPS counter.
        .arg("--no_track_gpu")
        .arg("--no_track_input")
        .arg("--no_track_display")
        .arg("--session_name").arg(&session_name)
        .arg("--stop_existing_session")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log_stderr))
        .creation_flags(CREATE_NO_WINDOW);

    log_event(&state.diagnostic_path, &format!(
        "Launching PresentMon with session={}", session_name
    ));
    log_event(&state.diagnostic_path, "Capture mode: parsing PresentMon v1 CSV directly from stdout");

    match command.spawn() {
        Ok(mut child) => {
            attach_to_job(&child, &state.diagnostic_path);
            let stats = Arc::new(SharedStats::default());
            if let Some(stdout) = child.stdout.take() {
                spawn_csv_reader(
                    stdout,
                    Arc::clone(&stats),
                    state.diagnostic_path.clone(),
                );
            } else {
                log_event(&state.diagnostic_path, "ERROR: PresentMon stdout pipe was not available");
            }

            state.capture = Some(Capture {
                child,
                process_name: process_name.to_string(),
                process_id,
                session_name: session_name.clone(),
                stats,
            });
            state.clear_capture_values("...");
            log_event(&state.diagnostic_path, "PresentMon child process spawned; reading CSV from stdout");
        }
        Err(e) => {
            state.clear_capture_values("ERR");
            log_event(&state.diagnostic_path, &format!("ERROR: could not launch PresentMon: {}", e));
        }
    }
}

pub(crate) fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => { field.push('"'); chars.next(); }
            '"' => quoted = !quoted,
            ',' if !quoted => { fields.push(std::mem::take(&mut field)); }
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}


pub(crate) fn find_metric_column(headers: &[String]) -> Option<(usize, &'static str)> {
    const METRICS: [&str; 4] = [
        "MsBetweenPresents",
        "MsBetweenDisplayChange",
        "MsBetweenAppStart",
        "MsBetweenSimulationStart",
    ];
    for metric in METRICS {
        if let Some(index) = headers.iter().position(|h| {
            h.trim().trim_start_matches('\u{feff}').eq_ignore_ascii_case(metric)
        }) {
            return Some((index, metric));
        }
    }
    None
}

// PresentMon 2.6 emits valid CSV rows to stdout, but live --output_file reads
// can fail on Windows with sharing violation 32. Parse stdout directly instead.
pub(crate) fn spawn_csv_reader<R: Read + Send + 'static>(
    stream: R,
    stats: Arc<SharedStats>,
    diagnostic_path: PathBuf,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut metric_index: Option<usize> = None;
        let mut expected_columns = 0usize;
        let mut intervals: Vec<f64> = Vec::with_capacity(30);
        // Recent frame times for the 1% low: the average of the slowest 1% of
        // the last ~600 frames, converted to FPS.
        let mut history: VecDeque<f64> = VecDeque::with_capacity(601);
        let mut last_low_calc = Instant::now();
        let mut first_fps_logged = false;
        let mut last_logged_fps = 0u32;
        let mut last_log_at = Instant::now();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    log_event(&diagnostic_path, &format!("ERROR reading PresentMon stdout: {}", e));
                    break;
                }
            }

            let line_text = line
                .trim_end_matches(['\r', '\n'])
                .trim_start_matches('\u{feff}');
            if line_text.trim().is_empty() {
                continue;
            }
            let fields = csv_fields(line_text);

            if metric_index.is_none() {
                if let Some((idx, name)) = find_metric_column(&fields) {
                    metric_index = Some(idx);
                    expected_columns = fields.len();
                    log_event(&diagnostic_path, &format!(
                        "PresentMon stdout CSV header recognized: {} at column {} ({} columns)",
                        name, idx, expected_columns
                    ));
                } else {
                    log_event(&diagnostic_path, &format!(
                        "WARNING: unrecognized PresentMon CSV header: {}",
                        line_text.chars().take(240).collect::<String>()
                    ));
                }
                continue;
            }

            if fields.len() != expected_columns {
                continue;
            }
            let Some(index) = metric_index else { continue; };
            let Some(raw_ms) = fields.get(index) else { continue; };
            let Ok(ms) = raw_ms.trim().parse::<f64>() else { continue; };
            if !(0.1..=1000.0).contains(&ms) {
                continue;
            }

            if let Ok(mut frames) = stats.frames.lock() {
                frames.push_back(ms as f32);
                while frames.len() > GRAPH_HISTORY { frames.pop_front(); }
            }
            intervals.push(ms);
            if intervals.len() > 30 {
                intervals.remove(0);
            }
            if intervals.len() < 2 {
                continue;
            }
            let average_ms = intervals.iter().sum::<f64>() / intervals.len() as f64;
            if average_ms <= 0.0 {
                continue;
            }
            let fps = (1000.0 / average_ms).round().clamp(1.0, 1000.0) as u32;
            stats.fps.store(fps, Ordering::Relaxed);
            stats.frametime_x100.store((average_ms * 100.0).round().clamp(0.0, 4_000_000.0) as u32, Ordering::Relaxed);
            stats.last_sample_ms.store(epoch_millis(), Ordering::Relaxed);

            history.push_back(ms);
            if history.len() > 600 { history.pop_front(); }
            if history.len() >= 60 && last_low_calc.elapsed() >= Duration::from_millis(250) {
                last_low_calc = Instant::now();
                let mut sorted: Vec<f64> = history.iter().copied().collect();
                sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                let count = (sorted.len() / 100).max(1);
                let worst_avg = sorted[..count].iter().sum::<f64>() / count as f64;
                if worst_avg > 0.0 {
                    stats.low1_fps.store((1000.0 / worst_avg).round().clamp(1.0, 1000.0) as u32, Ordering::Relaxed);
                }
            }

            if !first_fps_logged || fps.abs_diff(last_logged_fps) >= 5 || last_log_at.elapsed() >= Duration::from_secs(5) {
                log_event(&diagnostic_path, &format!(
                    "FPS parser updated: {} FPS from {} frame intervals (latest {:.3} ms)",
                    fps, intervals.len(), ms
                ));
                first_fps_logged = true;
                last_logged_fps = fps;
                last_log_at = Instant::now();
            }
        }
        log_event(&diagnostic_path, "PresentMon stdout CSV stream closed");
    });
}

