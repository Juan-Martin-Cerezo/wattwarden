//! Hardware-agnostic behaviour checked against two fake sysfs trees that replicate
//! the two real machines measured by Juan (both x86_64 Dells).
//!
//! |            | Dell Vostro        | Dell G15           |
//! |------------|--------------------|--------------------|
//! | cores      | 3                  | 20                 |
//! | freq       | 400000..1600000    | 400000..4600000    |
//! | RAPL PL1   | `max_power_uw` 15 W| `max_power_uw` 45 W|
//! | RAPL PL2   | not exposed (0)    | not exposed (0)    |
//! | backlight  | max 7500           | max 96000          |
//!
//! The rule under test: **every written value is derived from the discovered
//! hardware**, scales with it, and never lands above the ceiling the hardware
//! declares. There are no absolute fallbacks used for a write.
//!
//! Only Linux: the fake sysfs trees and the `wattwarden_platform::linux::*`
//! controllers under test do not exist on the other backends.
#![cfg(target_os = "linux")]

use std::fs;
use wattwarden_core::{CpuGovernor, DisplayManager, GpuController, RaplController};
use wattwarden_platform::linux::{
    LinuxBacklight, LinuxCpuGovernor, LinuxGpu, LinuxRapl, SysfsRoot,
};

const CPU: &str = "sys/devices/system/cpu";
const RAPL: &str = "sys/class/powercap/intel-rapl:0";
const DRM: &str = "sys/class/drm";
const BL: &str = "sys/class/backlight/intel_backlight";

/// Marker value left in `constraint_1_power_limit_uw`: it must never be rewritten.
const PL2_MARKER: &str = "1234000000";

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

/// Builds a fake Dell with the exact figures from the table.
fn machine(
    tag: &str,
    cpus: usize,
    min_khz: u64,
    max_khz: u64,
    pl1_max_uw: u64,
    bl_max: u64,
) -> SysfsRoot {
    let dir = std::env::temp_dir().join(format!("ww_hw_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);

    for i in 0..cpus {
        let base = format!("{CPU}/cpu{i}");
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_min_freq"),
            &format!("{min_khz}\n"),
        );
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_max_freq"),
            &format!("{max_khz}\n"),
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_min_freq"),
            &format!("{min_khz}\n"),
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_max_freq"),
            &format!("{max_khz}\n"),
        );
        write(
            &root,
            &format!("{base}/cpufreq/energy_performance_preference"),
            "balance_performance\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_governor"),
            "powersave\n",
        );
        if i > 0 {
            write(&root, &format!("{base}/online"), "1\n");
        }
    }
    write(&root, &format!("{CPU}/intel_pstate/no_turbo"), "0\n");

    // RAPL: PL1 declares its ceiling, PL2 reports 0 (not exposed) — exactly like both Dells.
    write(&root, &format!("{RAPL}/min_power_range_uw"), "0\n");
    write(
        &root,
        &format!("{RAPL}/max_power_range_uw"),
        &format!("{pl1_max_uw}\n"),
    );
    write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
    write(
        &root,
        &format!("{RAPL}/constraint_0_max_power_uw"),
        &format!("{pl1_max_uw}\n"),
    );
    write(
        &root,
        &format!("{RAPL}/constraint_0_power_limit_uw"),
        &format!("{pl1_max_uw}\n"),
    );
    write(&root, &format!("{RAPL}/constraint_1_name"), "short_term\n");
    write(&root, &format!("{RAPL}/constraint_1_max_power_uw"), "0\n");
    write(
        &root,
        &format!("{RAPL}/constraint_1_power_limit_uw"),
        &format!("{PL2_MARKER}\n"),
    );

    // GPU on card1, like both machines.
    write(&root, &format!("{DRM}/card1/gt_RPn_freq_mhz"), "300\n");
    write(&root, &format!("{DRM}/card1/gt_RP0_freq_mhz"), "1500\n");
    write(&root, &format!("{DRM}/card1/gt_min_freq_mhz"), "300\n");
    write(&root, &format!("{DRM}/card1/gt_max_freq_mhz"), "1500\n");

    // Backlight with the machine's own `max_brightness`.
    write(
        &root,
        &format!("{BL}/max_brightness"),
        &format!("{bl_max}\n"),
    );
    write(
        &root,
        &format!("{BL}/brightness"),
        &format!("{}\n", bl_max / 2),
    );

    root
}

fn fake_vostro(tag: &str) -> SysfsRoot {
    machine(tag, 3, 400_000, 1_600_000, 15_000_000, 7_500)
}

fn fake_g15(tag: &str) -> SysfsRoot {
    machine(tag, 20, 400_000, 4_600_000, 45_000_000, 96_000)
}

/// RAPL: each constraint clamps inside its **own** discovered range and PL2 (0 W,
/// not exposed) is never written. An impossible request (999 W) still lands exactly
/// on the hardware ceiling, never above `max_power_uw`.
#[test]
fn rapl_never_exceeds_each_constraints_own_ceiling() {
    for (tag, root, pl1_max_uw) in [
        ("vostro", fake_vostro("v"), 15_000_000u64),
        ("g15", fake_g15("g"), 45_000_000u64),
    ] {
        let rapl = LinuxRapl::with_root(root.clone()).unwrap();

        // The discovered ceiling is exactly what the machine declared.
        assert_eq!(
            rapl.pl1_bounds_watts(),
            Some((0, (pl1_max_uw / 1_000_000) as u32))
        );
        // PL2 has no range on either machine.
        assert_eq!(rapl.pl2_bounds_watts(), None);

        rapl.set_pl1_watts(999).unwrap();
        let written = read(&root, &format!("{RAPL}/constraint_0_power_limit_uw"));
        assert_eq!(
            written,
            pl1_max_uw.to_string(),
            "{tag}: PL1 must equal its ceiling"
        );
        assert!(
            written.parse::<u64>().unwrap() <= pl1_max_uw,
            "{tag}: PL1 must never exceed max_power_uw"
        );

        // PL2 not exposed -> the controller must not touch it.
        rapl.set_pl2_watts(999).unwrap();
        assert_eq!(
            read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
            PL2_MARKER,
            "{tag}: PL2 has no range and must not be written"
        );
    }
}

/// The same request scales inside each machine's own frequency and GPU range.
#[test]
fn freq_and_gpu_scale_with_each_machine() {
    let v = fake_vostro("v2");
    let g = fake_g15("g2");

    let vcpu = LinuxCpuGovernor::with_root(v.clone());
    let gcpu = LinuxCpuGovernor::with_root(g.clone());
    assert_eq!(vcpu.discovered_freq_bounds(), Some((400, 1600)));
    assert_eq!(gcpu.discovered_freq_bounds(), Some((400, 4600)));
    assert_eq!(vcpu.num_cpus(), 3);
    assert_eq!(gcpu.num_cpus(), 20);

    // 99999 MHz clamps to each machine's own ceiling, never beyond it.
    vcpu.set_freq_limit(99_999).unwrap();
    gcpu.set_freq_limit(99_999).unwrap();
    assert_eq!(
        read(&v, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
        "1600000"
    );
    assert_eq!(
        read(&g, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
        "4600000"
    );

    let vgpu = LinuxGpu::with_root(v.clone()).unwrap();
    let ggpu = LinuxGpu::with_root(g.clone()).unwrap();
    assert_eq!(vgpu.discovered_gpu_bounds(), Some((300, 1500)));
    vgpu.set_gpu_freq(99_999).unwrap();
    ggpu.set_gpu_freq(99_999).unwrap();
    assert_eq!(read(&v, &format!("{DRM}/card1/gt_max_freq_mhz")), "1500");
    assert_eq!(read(&g, &format!("{DRM}/card1/gt_max_freq_mhz")), "1500");
}

/// The ratchet test against the fake Vostro: the mutable `scaling_max_freq` was left
/// inflated at 2.4 GHz while the immutable `cpuinfo_max_freq` declares 1.6 GHz.
/// Discovery reads only `cpuinfo_*`, so the range stays 400..1600 and an impossible
/// request lands exactly on 1600000 — never on the inflated 2400000.
#[test]
fn inflated_scaling_max_never_widens_the_discovered_range() {
    let root = fake_vostro("ratchet");
    for i in 0..3 {
        write(
            &root,
            &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq"),
            "2400000\n",
        );
    }

    let cpu = LinuxCpuGovernor::with_root(root.clone());
    assert_eq!(cpu.discovered_freq_bounds(), Some((400, 1600)));
    assert_eq!(cpu.freq_bounds().unwrap(), (400, 1600));

    cpu.set_freq_limit(99_999).unwrap();
    for i in 0..3 {
        let written = read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq"));
        assert_eq!(written, "1600000", "cpu{i}: clamp to the real ceiling");
        assert!(
            written.parse::<u64>().unwrap() <= 1_600_000,
            "cpu{i}: never above the discovered max"
        );
    }
}

/// The same ratchet on the GPU: the writable `gt_max_freq_mhz` cannot define the
/// range. Only the immutable `gt_RPn`/`gt_RP0` do, so the request lands on 1500.
#[test]
fn gpu_discovery_uses_only_immutable_hw_info() {
    let root = fake_vostro("gpu_immutable");
    // Simulate a previous write having ratcheted the writable node down.
    write(&root, &format!("{DRM}/card1/gt_max_freq_mhz"), "700\n");

    let gpu = LinuxGpu::with_root(root.clone()).unwrap();
    assert_eq!(gpu.discovered_gpu_bounds(), Some((300, 1500)));
    gpu.set_gpu_freq(99_999).unwrap();
    assert_eq!(read(&root, &format!("{DRM}/card1/gt_max_freq_mhz")), "1500");
}

/// Brightness is always a percentage of the discovered `max_brightness`.
#[test]
fn backlight_percent_uses_each_discovered_max() {
    let v = fake_vostro("v3");
    let backlight = LinuxBacklight::with_root(v.clone()).unwrap();
    backlight.set_brightness_percent(50).unwrap();
    assert_eq!(
        read(&v, &format!("{BL}/brightness")),
        (7_500u64 * 50 / 100).to_string()
    );

    let g = fake_g15("g3");
    let backlight = LinuxBacklight::with_root(g.clone()).unwrap();
    backlight.set_brightness_percent(50).unwrap();
    assert_eq!(
        read(&g, &format!("{BL}/brightness")),
        (96_000u64 * 50 / 100).to_string()
    );
}

/// When a range cannot be discovered (missing/zero nodes), nothing is written.
#[test]
fn undiscoverable_ranges_are_never_written() {
    // A machine with cpufreq nodes but no `cpuinfo_*` and RAPL without any
    // `constraint_N_max_power_uw`.
    let dir = std::env::temp_dir().join(format!("ww_hw_nodisc_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);
    write(
        &root,
        &format!("{CPU}/cpu0/cpufreq/scaling_max_freq"),
        "800000\n",
    );
    write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
    write(
        &root,
        &format!("{RAPL}/constraint_0_power_limit_uw"),
        "9000000\n",
    );

    let cpu = LinuxCpuGovernor::with_root(root.clone());
    assert_eq!(cpu.discovered_freq_bounds(), None);
    cpu.set_freq_limit(1500).unwrap();
    assert_eq!(
        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
        "800000"
    );

    let rapl = LinuxRapl::with_root(root.clone()).unwrap();
    assert_eq!(rapl.pl1_bounds_watts(), None);
    rapl.set_pl1_watts(30).unwrap();
    assert_eq!(
        read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
        "9000000"
    );

    let _ = fs::remove_dir_all(&dir);
}
