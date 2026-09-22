//! The four machines the binary must boot on but was never run on: ARM/Raspberry Pi,
//! AMD, a desktop/server with no battery and a container.
//!
//! Every test builds a synthetic sysfs tree for one of those environments, boots the
//! real backend against it, runs the real daemon loop body (boot profile + adaptive
//! ladder + brightness loop) with every opt-in switch ON, and asserts the two
//! guarantees of `AGENTS.md`:
//!
//! 1. the daemon **does not fail** — every call returns, no panic, no `Err` that would
//!    abort startup;
//! 2. it **writes nothing that was not already there** — the set of nodes is identical
//!    before and after, so no absent RAPL/GPU/EPP/battery/backlight node is created and
//!    no Intel-only path is touched on non-Intel hardware.
//!
//! Linux only: the fake sysfs trees and the `linux::*` controllers under test do not
//! exist on the other backends.

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use wattwarden_core::{AutoExtremeLevel, Config, PowerProfile, PowerSource, RaplController};
use wattwarden_daemon::DaemonRunner;
use wattwarden_platform::{PlatformBackend, SysfsRoot};

const CPU: &str = "sys/devices/system/cpu";
const POWERCAP: &str = "sys/class/powercap";
const BAT: &str = "sys/class/power_supply/BAT0";
const BACKLIGHT: &str = "sys/class/backlight";

fn write(root: &SysfsRoot, rel: &str, value: &str) {
    let p = root.path(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, value).unwrap();
}

fn read(root: &SysfsRoot, rel: &str) -> String {
    fs::read_to_string(root.path(rel))
        .unwrap()
        .trim()
        .to_string()
}

fn mkdir(root: &SysfsRoot, rel: &str) {
    fs::create_dir_all(root.path(rel)).unwrap();
}

/// Every file under `base` with its raw bytes. The config file lives outside the fake
/// root, so only hardware nodes are captured.
fn snapshot(base: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut dirs = vec![base.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if let Ok(bytes) = fs::read(&path) {
                files.insert(path, bytes);
            }
        }
    }
    files
}

fn changed_paths(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> Vec<String> {
    before
        .iter()
        .filter(|(p, b)| after.get(*p) != Some(*b))
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect()
}

/// No node may appear or disappear: the daemon only ever writes to what the kernel
/// already exposes (a missing node has a missing parent, and the code probes for it).
fn assert_no_new_nodes(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
    tag: &str,
) {
    let created: Vec<&PathBuf> = after.keys().filter(|p| !before.contains_key(*p)).collect();
    assert!(
        created.is_empty(),
        "{tag}: the daemon created nodes that do not exist: {created:?}"
    );
    let removed: Vec<&PathBuf> = before.keys().filter(|p| !after.contains_key(*p)).collect();
    assert!(
        removed.is_empty(),
        "{tag}: the daemon removed nodes: {removed:?}"
    );
}

/// Backend + a config with every opt-in switch on, saved outside the fake root so the
/// boot profile AND the adaptive ladder really run.
fn everything_on(root: &SysfsRoot, tag: &str) -> (DaemonRunner, Config) {
    let backend = PlatformBackend::with_root(root.clone()).expect("the backend must boot");
    let cfg_dir = std::env::temp_dir().join(format!("ww_env_cfg_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cfg_dir);
    let cfg_path = cfg_dir.join("config.json");
    let cfg = Config {
        auto_extreme_enabled: true,
        auto_extreme_level: AutoExtremeLevel::Low,
        auto_brightness: true,
        profile: Some(PowerProfile::Performance),
        battery_charge_limit: Some(80),
        ..Default::default()
    };
    cfg.save(Some(&cfg_path)).expect("config must save");
    (DaemonRunner::with_paths(backend, Some(cfg_path), None), cfg)
}

/// The daemon loop body: boot settings, then the ladder at idle / half / full load,
/// then one brightness tick.
fn run_cycle(runner: &DaemonRunner, cfg: &Config, root: &SysfsRoot) {
    runner.apply_boot_settings(cfg);
    for load in ["0.00", "4.00", "100.00"] {
        write(
            root,
            "proc/loadavg",
            &format!("{load} {load} {load} 1/100 1234\n"),
        );
        runner.apply_logic_step(cfg);
    }
    runner.apply_brightness_step(Some("firefox"));
}

/// Raspberry Pi 5 (`aarch64`, `cpufreq-dt`): no powercap, no `intel_pstate`, no EPP,
/// no battery and no backlight device. This is the machine the evidence was measured
/// on.
fn arm_pi_tree(tag: &str) -> SysfsRoot {
    let dir = std::env::temp_dir().join(format!("ww_env_arm_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);

    for i in 0..4 {
        let base = format!("{CPU}/cpu{i}");
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_min_freq"),
            "1500000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_max_freq"),
            "2400000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_min_freq"),
            "1500000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_max_freq"),
            "2400000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_driver"),
            "cpufreq-dt\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_governor"),
            "ondemand\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_available_governors"),
            "conservative ondemand userspace powersave performance schedutil\n",
        );
        if i > 0 {
            write(&root, &format!("{base}/online"), "1\n");
        }
    }
    // The Pi's own turbo knob: `cpufreq/boost`, and no `intel_pstate` directory.
    write(&root, &format!("{CPU}/cpufreq/boost"), "0\n");

    // DRM cards exist (v3d / axi) but publish no GPU frequency node.
    mkdir(&root, "sys/class/drm/card0");
    mkdir(&root, "sys/class/drm/card1/card1-HDMI-A-1");
    write(&root, "sys/class/drm/version", "drm 1.1.0\n");

    // Backlight and power_supply classes are present but empty on the Pi.
    mkdir(&root, BACKLIGHT);
    mkdir(&root, "sys/class/power_supply");

    write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
    root
}

/// AMD desktop/laptop: `acpi-cpufreq` (`cpufreq/boost`, no `intel_pstate`), no EPP
/// interface, and a powercap zone named by the vendor rather than `intel-rapl:*`.
fn amd_tree(tag: &str) -> SysfsRoot {
    let dir = std::env::temp_dir().join(format!("ww_env_amd_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);

    for i in 0..8 {
        let base = format!("{CPU}/cpu{i}");
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_min_freq"),
            "2200000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_max_freq"),
            "4200000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_min_freq"),
            "2200000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_max_freq"),
            "4200000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_driver"),
            "acpi-cpufreq\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_governor"),
            "schedutil\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_available_governors"),
            "conservative ondemand userspace powersave performance schedutil\n",
        );
        // Deliberately no `energy_performance_preference` on any CPU.
        if i > 0 {
            write(&root, &format!("{base}/online"), "1\n");
        }
    }
    write(&root, &format!("{CPU}/cpufreq/boost"), "0\n");

    // Powercap exposes the RAPL zone under a non-Intel name and layout.
    let zone = format!("{POWERCAP}/amd-rapl:0");
    write(&root, &format!("{zone}/name"), "package-0\n");
    write(&root, &format!("{zone}/enabled"), "1\n");
    write(&root, &format!("{zone}/constraint_0_name"), "long_term\n");
    write(
        &root,
        &format!("{zone}/constraint_0_min_power_uw"),
        "2000000\n",
    );
    write(
        &root,
        &format!("{zone}/constraint_0_max_power_uw"),
        "65000000\n",
    );
    write(
        &root,
        &format!("{zone}/constraint_0_power_limit_uw"),
        "45000000\n",
    );

    mkdir(&root, BACKLIGHT);
    mkdir(&root, "sys/class/power_supply");
    write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
    root
}

/// Desktop / rack server: Intel CPU controls but no battery at all.
fn desktop_tree(tag: &str) -> SysfsRoot {
    let dir = std::env::temp_dir().join(format!("ww_env_desktop_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);

    for i in 0..8 {
        let base = format!("{CPU}/cpu{i}");
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_min_freq"),
            "800000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_max_freq"),
            "5000000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_min_freq"),
            "800000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_max_freq"),
            "5000000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_governor"),
            "powersave\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_available_governors"),
            "performance powersave\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/energy_performance_preference"),
            "balance_performance\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/energy_performance_available_preferences"),
            "default performance balance_performance balance_power power\n",
        );
        if i > 0 {
            write(&root, &format!("{base}/online"), "1\n");
        }
    }
    write(&root, &format!("{CPU}/intel_pstate/no_turbo"), "0\n");

    // `/sys/class/power_supply` exists (fan, AC…) but holds no battery.
    mkdir(&root, "sys/class/power_supply");
    write(&root, "sys/class/power_supply/AC/type", "Mains\n");
    write(&root, "sys/class/power_supply/AC/online", "1\n");

    write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
    root
}

/// Container / minimal root: no `/sys/class/backlight`, no powercap, no battery and,
/// in fact, no `/sys` tree at all.
fn container_tree(tag: &str) -> SysfsRoot {
    let dir = std::env::temp_dir().join(format!("ww_env_container_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);
    mkdir(&root, "sys");
    write(&root, "proc/loadavg", "0.00 0.00 0.00 1/100 1234\n");
    root
}

/// (a) ARM / Raspberry Pi.
#[test]
fn arm_pi_boots_without_rapl_battery_or_backlight() {
    let root = arm_pi_tree("boot");
    let backend = PlatformBackend::with_root(root.clone()).expect("the backend must boot");
    let caps = backend.capabilities();
    assert!(!caps.has_rapl, "the Pi has no RAPL");
    assert!(
        !caps.has_gpu_control,
        "the Pi exposes no GPU frequency node"
    );
    assert!(
        !caps.has_backlight_control,
        "the Pi has no backlight device"
    );
    assert!(!caps.has_battery, "the Pi has no battery");
    assert!(caps.is_stationary_mains);
    assert!(caps.has_cpu_frequency_control);
    assert!(!caps.has_charge_threshold);

    let (runner, cfg) = everything_on(&root, "arm_boot");
    let before = snapshot(root.root());
    run_cycle(&runner, &cfg, &root);
    let after = snapshot(root.root());

    assert_no_new_nodes(&before, &after, "ARM/Pi");
    assert!(
        !changed_paths(&before, &after).is_empty(),
        "ARM/Pi: the enabled ladder must actually write to the nodes it discovered"
    );
    // No Intel-only or absent node is invented.
    assert!(!root.exists(&format!("{CPU}/intel_pstate/no_turbo")));
    assert!(!root.exists(&format!("{CPU}/cpu0/cpufreq/energy_performance_preference")));
    assert!(!root.exists(POWERCAP));
    assert!(!root.exists(BAT));
    // What the Pi does expose was used: its boost knob and its governor.
    assert_eq!(root.read(&format!("{CPU}/cpufreq/boost")), "1");
    assert_eq!(
        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_governor")),
        "powersave"
    );
    // The discovered range, not an Intel default, bounds the frequency.
    let max_freq: u64 = read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq"))
        .parse()
        .unwrap();
    assert!(
        (1_500_000..=2_400_000).contains(&max_freq),
        "ARM/Pi: {max_freq} kHz is outside the discovered 1.5..2.4 GHz range"
    );
}

/// (b) AMD: a vendor-named powercap zone is discovered, the missing Intel nodes are
/// never written nor created.
#[test]
fn amd_vendor_named_powercap_is_discovered_and_intel_nodes_are_untouched() {
    let root = amd_tree("boot");
    let backend = PlatformBackend::with_root(root.clone()).expect("the backend must boot");

    let rapl = backend
        .rapl
        .as_ref()
        .expect("the amd-rapl zone must be found");
    assert_eq!(rapl.domains().len(), 1);
    assert_eq!(rapl.domains()[0], "sys/class/powercap/amd-rapl:0");
    assert_eq!(rapl.pl1_bounds_watts(), Some((2, 65)));
    assert_eq!(rapl.pl1_watts().unwrap(), 45);
    assert!(backend.capabilities().has_rapl);

    let (runner, cfg) = everything_on(&root, "amd_boot");
    let before = snapshot(root.root());
    run_cycle(&runner, &cfg, &root);
    let after = snapshot(root.root());

    assert_no_new_nodes(&before, &after, "AMD");
    // The zone was written inside its own discovered range, never a 115 W literal.
    let limit: u64 = read(
        &root,
        "sys/class/powercap/amd-rapl:0/constraint_0_power_limit_uw",
    )
    .parse()
    .unwrap();
    assert!(
        (2_000_000..=65_000_000).contains(&limit),
        "AMD: {limit} uW is outside the discovered 2..65 W range"
    );
    // Neither the Intel turbo knob nor the absent EPP interface appears.
    assert!(!root.exists(&format!("{CPU}/intel_pstate/no_turbo")));
    assert!(!root.exists(&format!("{CPU}/cpu0/cpufreq/energy_performance_preference")));
    // `acpi-cpufreq`'s own boost knob was used instead.
    assert_eq!(read(&root, &format!("{CPU}/cpufreq/boost")), "1");
}

/// (c) Desktop / server with no battery.
#[test]
fn desktop_without_battery_boots_and_writes_nothing_charge_related() {
    let root = desktop_tree("boot");
    let backend = PlatformBackend::with_root(root.clone()).expect("the backend must boot");
    let caps = backend.capabilities();
    assert!(!caps.has_battery);
    assert!(caps.is_stationary_mains);
    assert!(!caps.has_charge_threshold);

    // The source degrades to Stationary AC instead of failing: 0 % capacity, no
    // error, and the AC line state is still reported (Go `IsCharging` reads the
    // `Mains` supply's `online`, battery or not — hence charging = true here).
    assert!(backend.battery.is_stationary());
    assert_eq!(backend.battery.battery_percentage().unwrap(), 0);
    assert!(backend.battery.is_charging().unwrap());
    assert_eq!(
        backend.battery.consumption_watts().unwrap(),
        0.0,
        "no battery means no discharge wattage to report"
    );

    let (runner, cfg) = everything_on(&root, "desktop_boot");
    let before = snapshot(root.root());
    run_cycle(&runner, &cfg, &root);
    let after = snapshot(root.root());

    assert_no_new_nodes(&before, &after, "desktop");
    // In particular, requesting a charge limit on a battery-less machine (the config
    // asks for 80 %) must not conjure a battery node into existence.
    assert!(!root.exists(BAT));
    assert!(
        !after
            .keys()
            .any(|p| p.to_string_lossy().contains("power_supply/BAT")),
        "desktop: nothing under power_supply/BAT* may appear"
    );
}

/// (d) Container / minimal root.
#[test]
fn container_without_backlight_powercap_or_battery_boots() {
    let root = container_tree("boot");
    let backend = PlatformBackend::with_root(root.clone()).expect("the backend must boot");
    let caps = backend.capabilities();
    assert!(!caps.has_battery);
    assert!(caps.is_stationary_mains);
    assert!(!caps.has_rapl);
    assert!(!caps.has_gpu_control);
    assert!(!caps.has_backlight_control);
    assert!(!caps.has_charge_threshold);

    assert!(backend.rapl.is_none());
    assert!(backend.gpu.is_none());
    assert!(backend.backlight.is_none());

    let (runner, cfg) = everything_on(&root, "container_boot");
    let before = snapshot(root.root());
    run_cycle(&runner, &cfg, &root);
    let after = snapshot(root.root());

    assert_no_new_nodes(&before, &after, "container");
    // Nothing hardware-related exists, so the only file the cycle touched is the
    // `loadavg` the test itself rewrites: no control was attempted into the void.
    for path in changed_paths(&before, &after) {
        assert!(
            path.ends_with("proc/loadavg"),
            "container: unexpected write to {path}"
        );
    }
    // The classes the container does not mount were not created either.
    assert!(!root.exists(BACKLIGHT));
    assert!(!root.exists(POWERCAP));
    assert!(!root.exists(BAT));
}
