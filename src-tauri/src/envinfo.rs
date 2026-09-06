use std::process::Command;

use sysinfo::{Disks, System};

/// Best-effort OS/environment summary as markdown bullets (one fact per line).
/// Every source is optional: missing tools, files and env vars are skipped.
pub fn detect() -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(os_line());
    push_kernel(&mut lines);
    push_desktop_environment(&mut lines);
    push_package_manager(&mut lines);
    lines.push(shell_line());
    push_memory_and_cpu(&mut lines);
    for gpu in gpu_models() {
        lines.push(format!("- GPU: {}", gpu));
    }
    if let Some(model) = computer_model() {
        lines.push(format!("- Computer model: {}", model));
    }
    if let Some(host) = host_name() {
        lines.push(format!("- Hostname: {}", host));
    }
    if let Some(disk) = largest_disk() {
        lines.push(disk);
    }
    lines.join("\n")
}

fn cmd_output(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn file_content(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn human_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    }
}

fn os_line() -> String {
    let pretty = match std::env::consts::OS {
        "linux" => os_release_field("PRETTY_NAME").unwrap_or_else(|| "Linux".into()),
        "macos" => cmd_output("sw_vers", &["-productVersion"])
            .map(|v| format!("macOS {}", v))
            .unwrap_or_else(|| "macOS".into()),
        "windows" => {
            let name = System::name().unwrap_or_else(|| "Windows".into());
            match System::os_version() {
                Some(version) => format!("{} {}", name, version),
                None => name,
            }
        }
        other => other.to_string(),
    };
    format!("- OS: {} ({})", pretty, std::env::consts::ARCH)
}

fn os_release_field(field: &str) -> Option<String> {
    let prefix = format!("{}=", field);
    file_content("/etc/os-release")?
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty())
}

fn push_kernel(lines: &mut Vec<String>) {
    let version = System::kernel_version().or_else(|| {
        if cfg!(target_os = "linux") {
            cmd_output("uname", &["-r"])
        } else {
            None
        }
    });
    if let Some(version) = version {
        lines.push(format!("- Kernel: {}", version));
    }
}

fn push_desktop_environment(lines: &mut Vec<String>) {
    if !cfg!(target_os = "linux") {
        return;
    }
    if let Some(de) = env_value("XDG_CURRENT_DESKTOP") {
        lines.push(format!("- Desktop environment: {}", de));
    }
    if let Some(session) = env_value("DESKTOP_SESSION") {
        lines.push(format!("- Desktop session: {}", session));
    }
    if let Some(kind) = env_value("XDG_SESSION_TYPE") {
        lines.push(format!("- Session type: {}", kind));
    }
}

fn push_package_manager(lines: &mut Vec<String>) {
    if !cfg!(target_os = "linux") {
        return;
    }
    if let Some(manager) = package_manager() {
        lines.push(format!("- Package manager: {}", manager));
    }
    if let Some(distro) = os_release_field("ID") {
        lines.push(format!("- Distribution: {}", distro));
    }
}

fn package_manager() -> Option<&'static str> {
    const CANDIDATES: &[(&str, &[&str])] = &[
        ("pacman", &["pacman"]),
        ("dnf", &["dnf"]),
        ("yum", &["yum"]),
        ("zypper", &["zypper"]),
        ("apt", &["apt", "apt-get", "dpkg"]),
        ("emerge", &["emerge"]),
        ("xbps", &["xbps-install", "xbps-query"]),
        ("nix", &["nix-env", "nix-shell"]),
        ("apk", &["apk"]),
    ];
    CANDIDATES
        .iter()
        .find(|(_, binaries)| binaries.iter().any(|binary| in_path(binary)))
        .map(|(name, _)| *name)
}

fn in_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(binary).is_file()))
}

fn shell_line() -> String {
    if cfg!(windows) {
        let comspec = env_value("COMSPEC")
            .and_then(|path| {
                std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "powershell".into());
        let name = comspec.to_lowercase();
        if name.contains("powershell") || name.contains("pwsh") {
            "- Shell: powershell".into()
        } else if name.contains("cmd") {
            "- Shell: cmd".into()
        } else {
            format!("- Shell: {}", comspec)
        }
    } else {
        let path = env_value("SHELL").unwrap_or_else(|| "/bin/sh".into());
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        format!("- Shell: {} ({})", name, path)
    }
}

fn push_memory_and_cpu(lines: &mut Vec<String>) {
    let mut sys = System::new_all();
    sys.refresh_memory();

    lines.push(format!(
        "- RAM: {} ({} available)",
        human_bytes(sys.total_memory()),
        human_bytes(sys.available_memory())
    ));
    if sys.total_swap() > 0 {
        lines.push(format!("- Swap: {}", human_bytes(sys.total_swap())));
    }

    let cpus = sys.cpus();
    if let Some(cpu) = cpus.first() {
        let brand = cpu.brand().trim();
        if brand.is_empty() {
            lines.push(format!("- CPU: {} logical cores", cpus.len()));
        } else {
            lines.push(format!("- CPU: {} ({} logical cores)", brand, cpus.len()));
        }
        let vendor = cpu.vendor_id().trim();
        if !vendor.is_empty() {
            lines.push(format!("- CPU vendor: {}", vendor));
        }
        if let Some(physical) = sys.physical_core_count() {
            lines.push(format!("- CPU physical cores: {}", physical));
        }
        if cpu.frequency() > 0 {
            lines.push(format!("- CPU frequency: {} MHz", cpu.frequency()));
        }
    }
}

fn gpu_models() -> Vec<String> {
    if cfg!(target_os = "linux") {
        return cmd_output("lspci", &["-nn"])
            .map(|out| {
                out.lines()
                    .filter(|line| {
                        let line = line.to_lowercase();
                        line.contains("vga") || line.contains("3d") || line.contains("display")
                    })
                    .filter_map(lspci_gpu_description)
                    .collect()
            })
            .unwrap_or_default();
    }
    if cfg!(target_os = "macos") {
        return cmd_output("system_profiler", &["SPDisplaysDataType"])
            .map(|out| {
                out.lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        if line.contains("Chipset") || line.contains("Vendor") {
                            line.split_once(':')
                                .map(|(_, rest)| rest.trim().to_string())
                                .filter(|rest| !rest.is_empty())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    if cfg!(windows) {
        return cmd_output(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ],
        )
        .map(|out| {
            out.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    }
    Vec::new()
}

fn lspci_gpu_description(line: &str) -> Option<String> {
    let rest = match line.split_once("]: ") {
        Some((_, rest)) => rest,
        None => line.split_once(':')?.1,
    };
    let rest = rest.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn computer_model() -> Option<String> {
    if cfg!(target_os = "linux") {
        let name = file_content("/sys/class/dmi/id/product_name")
            .or_else(|| file_content("/sys/devices/virtual/dmi/id/product_name"))?;
        let vendor = file_content("/sys/class/dmi/id/product_vendor")
            .or_else(|| file_content("/sys/devices/virtual/dmi/id/product_vendor"));
        Some(match vendor {
            Some(vendor) if !name.contains(&vendor) => format!("{} {}", vendor, name),
            _ => name,
        })
    } else if cfg!(target_os = "macos") {
        cmd_output("sysctl", &["-n", "hw.model"])
    } else if cfg!(windows) {
        cmd_output(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_ComputerSystem | ForEach-Object { \"$($_.Manufacturer) $($_.Model)\".Trim() }",
            ],
        )
    } else {
        None
    }
}

fn host_name() -> Option<String> {
    System::host_name()
        .or_else(|| env_value("COMPUTERNAME"))
        .or_else(|| env_value("HOSTNAME"))
}

fn largest_disk() -> Option<String> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|disk| disk.total_space() > 0)
        .max_by_key(|disk| disk.total_space())
        .map(|disk| {
            format!(
                "- Disk: {} total ({})",
                human_bytes(disk.total_space()),
                disk.mount_point().display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_is_a_bullet_list() {
        let out = detect();
        assert!(out.starts_with("- OS: "));
        assert!(!out.contains("\n\n"));
    }

    #[test]
    fn human_bytes_formats_gib_and_mib() {
        assert_eq!(human_bytes(0), "0.0 MiB");
        assert_eq!(human_bytes(512 * 1024 * 1024), "512.0 MiB");
        assert_eq!(human_bytes(32 * 1024 * 1024 * 1024), "32.0 GiB");
    }
}
