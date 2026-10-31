use std::process::Command;
use sysinfo::{Disks, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    None,
}

impl GpuVendor {
    pub fn label(self) -> &'static str {
        match self {
            GpuVendor::Nvidia => "NVIDIA",
            GpuVendor::Amd => "AMD",
            GpuVendor::Intel => "Intel",
            GpuVendor::Apple => "Apple",
            GpuVendor::None => "none",
        }
    }
}

/// How much we trust a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
    Unknown,
}

impl Confidence {
    pub fn label(self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
            Confidence::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hardware {
    pub os: String,
    pub os_family: String,
    pub arch: String,
    pub cpu: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub ram_gb: f64,
    pub ram_available_gb: f64,
    pub disk_free_gb: f64,
    pub gpu_vendor: GpuVendor,
    pub gpu_name: String,
    /// GPU memory available to models. On Apple Silicon this is an estimated share of unified memory.
    pub vram_gb: Option<f64>,
    pub vram_source: String,
    pub vram_confidence: Confidence,
    pub ollama: bool,
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

/// Parses `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`.
/// With several GPUs, returns the one with the most memory.
pub fn parse_nvidia_smi(text: &str) -> Option<(String, f64)> {
    text.lines()
        .filter_map(|line| {
            let (name, mib) = line.rsplit_once(',')?;
            Some((name.trim().to_string(), mib.trim().parse::<f64>().ok()? / 1024.0))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// Parses `rocm-smi --showmeminfo vram --csv` output (bytes).
pub fn parse_rocm_smi(text: &str) -> Option<f64> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Vec<&str> = lines.next()?.split(',').collect();
    let idx = header.iter().position(|h| h.contains("Total Memory"))?;
    lines
        .filter_map(|l| l.split(',').nth(idx)?.trim().parse::<f64>().ok())
        .map(|b| b / 1024f64.powi(3))
        .reduce(f64::max)
}

fn other_gpu_name() -> Option<String> {
    let text = if cfg!(target_os = "linux") {
        run("lspci", &[])?
            .lines()
            .filter(|l| l.contains("VGA") || l.contains("3D controller") || l.contains("Display controller"))
            .map(|l| l.split_once(": ").map(|x| x.1).unwrap_or(l).to_string())
            .collect::<Vec<_>>()
            .join("\n")
    } else if cfg!(windows) {
        run(
            "powershell",
            &["-NoProfile", "-Command", "(Get-CimInstance Win32_VideoController).Name"],
        )?
    } else if cfg!(target_os = "macos") {
        run("system_profiler", &["SPDisplaysDataType"])?
            .lines()
            .filter_map(|l| l.trim().strip_prefix("Chipset Model:").map(|s| s.trim().to_string()))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        return None;
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

pub fn vendor_from_name(name: &str) -> GpuVendor {
    let n = name.to_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") {
        GpuVendor::Nvidia
    } else if n.contains("amd") || n.contains("radeon") || n.contains("advanced micro devices") {
        GpuVendor::Amd
    } else if n.contains("intel") {
        GpuVendor::Intel
    } else {
        GpuVendor::None
    }
}

fn disk_free_for(path: &std::path::Path) -> f64 {
    let disks = Disks::new_with_refreshed_list();
    let best = disks
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .or_else(|| disks.iter().max_by_key(|d| d.available_space()));
    best.map(|d| d.available_space() as f64 / 1024f64.powi(3))
        .unwrap_or(0.0)
}

pub fn free_disk_gb(path: &std::path::Path) -> f64 {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            break;
        }
    }
    disk_free_for(&p)
}

pub fn detect() -> Hardware {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let gib = 1024f64.powi(3);
    let ram_gb = sys.total_memory() as f64 / gib;

    let mut hw = Hardware {
        os: System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into()),
        os_family: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        cpu: sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default(),
        cpu_cores: System::physical_core_count().unwrap_or(1),
        cpu_threads: sys.cpus().len(),
        ram_gb: round1(ram_gb),
        ram_available_gb: round1(sys.available_memory() as f64 / gib),
        disk_free_gb: round1(free_disk_gb(&super::paths::home())),
        gpu_vendor: GpuVendor::None,
        gpu_name: String::new(),
        vram_gb: None,
        vram_source: "none".into(),
        vram_confidence: Confidence::Unknown,
        ollama: super::paths::which("ollama").is_some(),
    };

    if hw.os_family == "macos" && hw.arch == "aarch64" {
        hw.gpu_vendor = GpuVendor::Apple;
        hw.gpu_name = "Silicon GPU (Metal, unified memory)".into();
        hw.vram_gb = Some(round1(ram_gb * apple_share(ram_gb)));
        hw.vram_source = "estimated share of unified memory".into();
        hw.vram_confidence = Confidence::Medium;
    } else if let Some((name, vram)) = run(
        "nvidia-smi",
        &["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"],
    )
    .as_deref()
    .and_then(parse_nvidia_smi)
    {
        hw.gpu_vendor = GpuVendor::Nvidia;
        hw.gpu_name = name;
        hw.vram_gb = Some(round1(vram));
        hw.vram_source = "nvidia-smi".into();
        hw.vram_confidence = Confidence::High;
    } else if let Some(name) = other_gpu_name() {
        let first = name
            .lines()
            .find(|l| vendor_from_name(l) != GpuVendor::None)
            .unwrap_or("");
        let vendor = vendor_from_name(first);
        if vendor != GpuVendor::None {
            hw.gpu_vendor = vendor;
            hw.gpu_name = first.trim().to_string();
            if vendor == GpuVendor::Amd {
                if let Some(v) = run("rocm-smi", &["--showmeminfo", "vram", "--csv"])
                    .as_deref()
                    .and_then(parse_rocm_smi)
                {
                    hw.vram_gb = Some(round1(v));
                    hw.vram_source = "rocm-smi".into();
                    hw.vram_confidence = Confidence::High;
                }
            }
            if hw.vram_gb.is_none() {
                hw.vram_source = "could not be read on this system".into();
            }
        }
    }
    hw
}

/// Apple lets the GPU use about 65-75% of unified memory.
fn apple_share(ram_gb: f64) -> f64 {
    if ram_gb >= 32.0 {
        0.75
    } else {
        0.65
    }
}

/// Memory budget used for models, in GB.
pub fn budget_gb(hw: &Hardware) -> f64 {
    match (hw.gpu_vendor, hw.vram_gb) {
        (GpuVendor::None, _) | (_, None) => round1(hw.ram_gb * 0.5),
        (_, Some(v)) => v,
    }
}

/// True when the model will run on the CPU (no usable GPU memory known).
pub fn cpu_only(hw: &Hardware) -> bool {
    hw.gpu_vendor == GpuVendor::None || hw.vram_gb.is_none()
}

#[cfg(test)]
pub mod fixtures {
    use super::*;

    fn base() -> Hardware {
        Hardware {
            os: "test".into(),
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cpu: "Test CPU".into(),
            cpu_cores: 8,
            cpu_threads: 16,
            ram_gb: 16.0,
            ram_available_gb: 10.0,
            disk_free_gb: 200.0,
            gpu_vendor: GpuVendor::None,
            gpu_name: String::new(),
            vram_gb: None,
            vram_source: "none".into(),
            vram_confidence: Confidence::Unknown,
            ollama: false,
        }
    }

    pub fn cpu_only_8gb() -> Hardware {
        Hardware { ram_gb: 8.0, ..base() }
    }

    pub fn nvidia_8gb() -> Hardware {
        Hardware {
            gpu_vendor: GpuVendor::Nvidia,
            gpu_name: "RTX 4060".into(),
            vram_gb: Some(8.0),
            vram_confidence: Confidence::High,
            ..base()
        }
    }

    pub fn nvidia_24gb() -> Hardware {
        Hardware {
            ram_gb: 64.0,
            gpu_vendor: GpuVendor::Nvidia,
            gpu_name: "RTX 4090".into(),
            vram_gb: Some(24.0),
            vram_confidence: Confidence::High,
            ..base()
        }
    }

    pub fn apple_16gb() -> Hardware {
        Hardware {
            os_family: "macos".into(),
            arch: "aarch64".into(),
            gpu_vendor: GpuVendor::Apple,
            vram_gb: Some(10.4),
            vram_confidence: Confidence::Medium,
            ..base()
        }
    }

    pub fn all() -> Vec<(&'static str, Hardware)> {
        vec![
            ("cpu-8gb", cpu_only_8gb()),
            ("nvidia-8gb", nvidia_8gb()),
            ("nvidia-24gb", nvidia_24gb()),
            ("apple-16gb", apple_16gb()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_single_and_multi() {
        let (n, v) = parse_nvidia_smi("NVIDIA GeForce RTX 4060, 8188\n").unwrap();
        assert_eq!(n, "NVIDIA GeForce RTX 4060");
        assert!((v - 8.0).abs() < 0.01);
        let (n, _) = parse_nvidia_smi("GPU A, 4096\nGPU B, 24576\n").unwrap();
        assert_eq!(n, "GPU B");
        assert!(parse_nvidia_smi("garbage").is_none());
    }

    #[test]
    fn rocm_smi_csv() {
        let text = "device,VRAM Total Memory (B),VRAM Total Used Memory (B)\ncard0,17163091968,1000\n";
        let v = parse_rocm_smi(text).unwrap();
        assert!((v - 15.98).abs() < 0.01);
    }

    #[test]
    fn budgets() {
        assert_eq!(budget_gb(&fixtures::cpu_only_8gb()), 4.0);
        assert_eq!(budget_gb(&fixtures::nvidia_8gb()), 8.0);
    }

    #[test]
    fn vendor_names() {
        assert_eq!(vendor_from_name("AMD Radeon RX 7800"), GpuVendor::Amd);
        assert_eq!(vendor_from_name("Intel(R) UHD Graphics"), GpuVendor::Intel);
        assert_eq!(vendor_from_name("Microsoft Basic Display"), GpuVendor::None);
    }
}
