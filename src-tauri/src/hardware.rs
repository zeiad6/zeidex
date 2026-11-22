//! The hardware probe behind the AI & OCR Hardware Advisor.
//!
//! Everything measured here is reported as measured: a figure the platform does
//! not expose stays zero and the UI says "unknown" instead of inventing a
//! number. The classification and the recommendation are pure functions in
//! `zeidex-models`; this module only fills their input.

use std::path::Path;

use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};
use zeidex_models::DeviceProfile;

/// Samples the machine. `models_root` selects the volume whose free space
/// matters: the one packs are installed to.
pub fn probe(models_root: &Path) -> DeviceProfile {
    let os = System::long_os_version()
        .ok()
        .or_else(|| System::name().ok())
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    let arch = System::cpu_arch();
    let free_disk_mb = free_space_mb(models_root);
    let (gpu, gpu_memory_mb) = detect_gpu();

    let Ok(mut system) = System::new_with_specifics(
        RefreshKind::nothing()
            .with_memory(MemoryRefreshKind::everything())
            .with_cpu(CpuRefreshKind::nothing().with_frequency()),
    ) else {
        // The probe failing is not a reason to block OCR: an empty profile
        // classifies as Basic and the recommendation falls back to Compact.
        return DeviceProfile {
            os,
            arch,
            free_disk_mb,
            gpu,
            gpu_memory_mb,
            ..DeviceProfile::default()
        };
    };
    // A second memory refresh is cheap and catches a value that changed while
    // the CPU list was being read.
    system.refresh_memory();

    let cpus = system.cpus();
    let cpu_model = cpus
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|brand| !brand.is_empty())
        .unwrap_or_default();
    let logical_cores = cpus.len().max(
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
    );
    let physical_cores = System::physical_core_count().unwrap_or(0);
    // The build's own capability and the probe must both say yes: a CPU-only
    // build on a machine with a GPU still runs on the CPU.
    let gpu_acceleration = cfg!(any(feature = "cuda", feature = "directml")) && gpu.is_some();

    DeviceProfile {
        cpu_model,
        physical_cores,
        logical_cores,
        total_ram_mb: system.total_memory() / (1024 * 1024),
        available_ram_mb: system.available_memory() / (1024 * 1024),
        free_disk_mb,
        os,
        arch,
        gpu,
        gpu_memory_mb,
        gpu_acceleration,
    }
}

/// Free space in MiB on the filesystem that holds `path`.
///
/// `path` may not exist yet (a fresh install creates the models folder on the
/// first download), so the nearest existing ancestor is measured instead.
/// `0` means "could not measure", which the advisor treats as unknown rather
/// than as a full disk.
pub fn free_space_mb(path: &Path) -> u64 {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            return 0;
        }
    }
    let probe = probe.canonicalize().unwrap_or(probe);
    let Ok(disks) = Disks::new_with_refreshed_list() else {
        return 0;
    };
    let mut best: Option<(usize, u64)> = None;
    for disk in disks.iter() {
        let mount = disk.mount_point();
        if probe.starts_with(mount) {
            let depth = mount.components().count();
            if best.is_none_or(|(seen, _)| depth > seen) {
                best = Some((depth, disk.available_space()));
            }
        }
    }
    best.map(|(_, bytes)| bytes / (1024 * 1024)).unwrap_or(0)
}

/// Resident memory of this process in MiB, for the benchmark's memory figure.
pub fn process_memory_mb() -> u64 {
    let Ok(pid) = sysinfo::get_current_pid() else {
        return 0;
    };
    let Ok(mut system) = System::new() else {
        return 0;
    };
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .map(|process| process.memory() / (1024 * 1024))
        .unwrap_or(0)
}

/// GPU name and memory when the platform exposes them without a driver query.
///
/// Linux exposes adapters through sysfs, which is a read of world-readable
/// files: no ioctl, no driver initialisation, nothing that can hang a probe.
/// Other platforms return nothing rather than shelling out to a vendor tool.
#[cfg(target_os = "linux")]
fn detect_gpu() -> (Option<String>, Option<u64>) {
    let mut found: Option<(String, Option<u64>)> = None;
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return (None, None);
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // "card0" is an adapter; "card0-DP-1" is a connector and "renderD128"
        // is a node, neither of which describes the hardware twice.
        let Some(suffix) = name.strip_prefix("card") else {
            continue;
        };
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let device = entry.path().join("device");
        let vendor = std::fs::read_to_string(device.join("vendor"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let vendor_name = match vendor.as_str() {
            "0x10de" => "NVIDIA",
            "0x1002" => "AMD",
            "0x8086" => "Intel",
            "0x13b5" => "ARM",
            _ => continue,
        };
        let mut model = vendor_name.to_string();
        let mut memory_mb = None;

        // amdgpu publishes VRAM in sysfs, in bytes.
        if let Ok(raw) = std::fs::read_to_string(device.join("mem_info_vram_total"))
            && let Ok(bytes) = raw.trim().parse::<u64>()
        {
            memory_mb = Some(bytes / (1024 * 1024));
        }
        // The NVIDIA driver publishes the board name and VRAM in procfs.
        if vendor == "0x10de"
            && let Ok(gpus) = std::fs::read_dir("/proc/driver/nvidia/gpus")
        {
            for gpu in gpus.flatten() {
                let Ok(info) = std::fs::read_to_string(gpu.path().join("information")) else {
                    continue;
                };
                for line in info.lines() {
                    if let Some(rest) = line.strip_prefix("Model:") {
                        model = format!("NVIDIA {}", rest.trim());
                    } else if let Some(rest) = line.strip_prefix("Video Memory:") {
                        // "Video Memory: 8192 MiB"
                        memory_mb = rest
                            .split_whitespace()
                            .next()
                            .and_then(|value| value.parse::<u64>().ok());
                    }
                }
            }
        }
        found = Some((model, memory_mb));
        break;
    }
    match found {
        Some((model, memory)) => (Some(model), memory),
        None => (None, None),
    }
}

#[cfg(not(target_os = "linux"))]
fn detect_gpu() -> (Option<String>, Option<u64>) {
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_probe_reports_plausible_figures() {
        let profile = probe(Path::new("."));
        assert!(!profile.os.is_empty());
        assert!(!profile.arch.is_empty());
        assert!(profile.logical_cores >= 1);
        assert!(
            profile.total_ram_mb > 0,
            "sysinfo should read installed RAM on every supported platform"
        );
        assert!(profile.available_ram_mb <= profile.total_ram_mb);
    }

    #[test]
    fn free_space_is_measured_for_an_existing_ancestor() {
        let directory = std::env::temp_dir();
        assert!(
            free_space_mb(&directory.join("zeidex-not-created-yet")) > 0,
            "the nearest existing ancestor should still be measurable"
        );
    }
}
