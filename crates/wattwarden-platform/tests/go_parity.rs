//! Paridad Go→Rust verificada contra un sysfs FALSO de laptop Intel.
//!
//! Estos tests son el criterio de aceptación de `PARITY.md` §5: afirman los valores
//! EXACTOS que escribe el binario Go de `master`, sin root, sin hardware real y sin
//! binarios externos. Todo pasa por `WATTWARDEN_SYSFS_ROOT` vía `SysfsRoot::new(dir)`.

use std::fs;
use std::path::PathBuf;
use wattwarden_core::{
    AspmController, CpuGovernor, DisplayManager, GpuController, PeripheralsController, PowerSource,
    RaplController, SystemTweaksController,
};
use wattwarden_platform::linux::{
    LinuxAspm, LinuxBacklight, LinuxGpu, LinuxPeripherals, LinuxRapl, LinuxSystemTweaks, SysfsRoot,
};

const CPU: &str = "sys/devices/system/cpu";
const RAPL: &str = "sys/class/powercap/intel-rapl:0";
const DRM: &str = "sys/class/drm";
const BL: &str = "sys/class/backlight/intel_backlight";
const BAT: &str = "sys/class/power_supply/BAT0";

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

/// Laptop Intel sintética: 8 CPUs, RAPL 2..60 W, GPU 300..1100 MHz, backlight 1000, BAT0.
fn fake_intel_laptop(tag: &str) -> SysfsRoot {
    let dir = std::env::temp_dir().join(format!("ww_parity_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let root = SysfsRoot::new(&dir);

    // --- CPU: 8 cores con cpufreq (kHz en sysfs, igual que el kernel real) ---
    for i in 0..8 {
        let base = format!("{CPU}/cpu{i}");
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_min_freq"),
            "400000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/cpuinfo_max_freq"),
            "3500000\n",
        );
        write(
            &root,
            &format!("{base}/cpufreq/scaling_max_freq"),
            "3500000\n",
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

    // --- RAPL: 2 W .. 60 W, constraints por nombre ---
    write(&root, &format!("{RAPL}/min_power_range_uw"), "2000000\n");
    write(&root, &format!("{RAPL}/max_power_range_uw"), "60000000\n");
    write(&root, &format!("{RAPL}/constraint_0_name"), "long_term\n");
    write(&root, &format!("{RAPL}/constraint_1_name"), "short_term\n");
    write(
        &root,
        &format!("{RAPL}/constraint_0_power_limit_uw"),
        "45000000\n",
    );
    write(
        &root,
        &format!("{RAPL}/constraint_1_power_limit_uw"),
        "60000000\n",
    );

    // --- GPU integrada en card0 ---
    write(&root, &format!("{DRM}/card0/gt_max_freq_mhz"), "1100\n");
    write(&root, &format!("{DRM}/card0/gt_RPn_freq_mhz"), "300\n");
    write(&root, &format!("{DRM}/card0/gt_RP0_freq_mhz"), "1100\n");
    write(&root, &format!("{DRM}/card0/gt_min_freq_mhz"), "300\n");

    // --- Backlight ---
    write(&root, &format!("{BL}/max_brightness"), "1000\n");
    write(&root, &format!("{BL}/brightness"), "500\n");

    // --- Batería ---
    write(&root, &format!("{BAT}/type"), "Battery\n");
    write(&root, &format!("{BAT}/capacity"), "72\n");
    write(&root, &format!("{BAT}/status"), "Discharging\n");
    write(&root, "sys/class/power_supply/AC/type", "Mains\n");
    write(&root, "sys/class/power_supply/AC/online", "0\n");

    // --- Periféricos / tweaks ---
    write(
        &root,
        "sys/class/leds/tpacpi::kbd_backlight/brightness",
        "0\n",
    );
    write(
        &root,
        "sys/class/leds/tpacpi::kbd_backlight/max_brightness",
        "2\n",
    );
    write(
        &root,
        "sys/module/snd_hda_intel/parameters/power_save",
        "0\n",
    );
    write(
        &root,
        "sys/module/snd_hda_intel/parameters/power_save_controller",
        "N\n",
    );
    write(&root, "sys/module/iwlwifi/parameters/power_save", "N\n");
    write(&root, "sys/bus/usb/devices/usb1/power/control", "on\n");
    write(
        &root,
        "sys/bus/pci/devices/0000:00:14.0/power/control",
        "on\n",
    );
    write(&root, "proc/sys/kernel/nmi_watchdog", "1\n");
    write(&root, "proc/sys/vm/dirty_writeback_centisecs", "500\n");
    write(
        &root,
        "sys/module/pcie_aspm/parameters/policy",
        "default [powersave] performance\n",
    );

    root
}

// --- (a) + (b) freq bounds y SetFreqLimit (Go: clamp a bounds, min+max en TODOS los cpus) ---
#[test]
fn a_b_freq_bounds_and_limit_match_go() {
    let root = fake_intel_laptop("freq");
    let cpu = wattwarden_platform::linux::LinuxCpuGovernor::with_root(root.clone());

    assert_eq!(cpu.freq_bounds().unwrap(), (400, 3500));

    cpu.set_freq_limit(3000).unwrap();
    for i in 0..8 {
        assert_eq!(
            read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_min_freq")),
            "400000"
        );
        assert_eq!(
            read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_max_freq")),
            "3000000"
        );
    }

    // 99999 clampa al hw max (Go ApplyModePerformance), 100 clampa al hw min.
    cpu.set_freq_limit(99_999).unwrap();
    assert_eq!(
        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
        "3500000"
    );
    cpu.set_freq_limit(100).unwrap();
    assert_eq!(
        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
        "400000"
    );

    assert_eq!(cpu.freq_limit().unwrap(), 400);

    let _ = fs::remove_dir_all(root.root());
}

// --- (c) SetCores: cpu1..cpu2 en 1, cpu3..cpu7 en 0, y re-aplica el freq limit ---
#[test]
fn c_set_online_cores_matches_go_including_freq_reapply() {
    let root = fake_intel_laptop("cores");
    let cpu = wattwarden_platform::linux::LinuxCpuGovernor::with_root(root.clone());

    cpu.set_freq_limit(2000).unwrap();
    // Simula que algo pisó el limite antes de encender/apagar cores.
    write(
        &root,
        &format!("{CPU}/cpu3/cpufreq/scaling_max_freq"),
        "3500000\n",
    );

    cpu.set_online_cores(3).unwrap();

    assert_eq!(read(&root, &format!("{CPU}/cpu1/online")), "1");
    assert_eq!(read(&root, &format!("{CPU}/cpu2/online")), "1");
    for i in 3..8 {
        assert_eq!(read(&root, &format!("{CPU}/cpu{i}/online")), "0");
    }
    // Go re-aplica SetFreqLimit(GetFreqLimit()) al final de SetCores.
    assert_eq!(
        read(&root, &format!("{CPU}/cpu3/cpufreq/scaling_max_freq")),
        "2000000"
    );
    assert_eq!(cpu.online_cores().unwrap(), 3);

    let _ = fs::remove_dir_all(root.root());
}

// --- (d) RAPL: descubre el constraint por NOMBRE y escribe W*1e6 clampeado ---
#[test]
fn d_rapl_discovers_by_name_and_writes_microwatts() {
    let root = fake_intel_laptop("rapl");
    let rapl = LinuxRapl::with_root(root.clone()).unwrap();

    assert_eq!(rapl.rapl_bounds().unwrap(), (2, 60));
    assert_eq!(rapl.pl1_watts().unwrap(), 45);
    assert_eq!(rapl.pl2_watts().unwrap(), 60);

    rapl.set_pl1_watts(30).unwrap();
    assert_eq!(
        read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
        "30000000"
    );
    assert_eq!(
        read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
        "60000000"
    );

    // 200 W clampa al max del rango hw; 1 W clampa al min.
    rapl.set_pl2_watts(200).unwrap();
    assert_eq!(
        read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
        "60000000"
    );
    rapl.set_pl2_watts(1).unwrap();
    assert_eq!(
        read(&root, &format!("{RAPL}/constraint_1_power_limit_uw")),
        "2000000"
    );

    let _ = fs::remove_dir_all(root.root());
}

// --- (e) GPU: card1 preferida, bounds con fallbacks, orden min→max ---
#[test]
fn e_gpu_bounds_and_write_order_match_go() {
    let root = fake_intel_laptop("gpu");
    let gpu = LinuxGpu::with_root(root.clone()).unwrap();
    assert_eq!(gpu.gpu_bounds().unwrap(), (300, 1100));

    gpu.set_gpu_freq(900).unwrap();
    assert_eq!(read(&root, &format!("{DRM}/card0/gt_min_freq_mhz")), "300");
    assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "900");

    gpu.set_gpu_freq(99_999).unwrap();
    assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "1100");
    gpu.set_gpu_freq(10).unwrap();
    assert_eq!(read(&root, &format!("{DRM}/card0/gt_max_freq_mhz")), "300");

    // Si aparece card1 (dGPU), gana card1 — exactamente el orden de Go.
    write(&root, &format!("{DRM}/card1/gt_max_freq_mhz"), "1500\n");
    let gpu2 = LinuxGpu::with_root(root.clone()).unwrap();
    assert_eq!(gpu2.gpu_bounds().unwrap(), (300, 1100)); // RPn/RP0 ausentes -> fallback

    let _ = fs::remove_dir_all(root.root());
}

// --- (f) EPP escribe la preferencia en TODOS los cpus + governor derivado ---
#[test]
fn f_epp_writes_preference_and_governor_on_every_cpu() {
    let root = fake_intel_laptop("epp");
    let cpu = wattwarden_platform::linux::LinuxCpuGovernor::with_root(root.clone());

    cpu.set_energy_performance_preference("performance")
        .unwrap();
    for i in 0..8 {
        assert_eq!(
            read(
                &root,
                &format!("{CPU}/cpu{i}/cpufreq/energy_performance_preference")
            ),
            "performance"
        );
        assert_eq!(
            read(&root, &format!("{CPU}/cpu{i}/cpufreq/scaling_governor")),
            "performance"
        );
    }

    // Cualquier otra preferencia (incluido "power") deja el governor en powersave.
    cpu.set_energy_performance_preference("power").unwrap();
    assert_eq!(
        read(
            &root,
            &format!("{CPU}/cpu0/cpufreq/energy_performance_preference")
        ),
        "power"
    );
    assert_eq!(
        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_governor")),
        "powersave"
    );

    let _ = fs::remove_dir_all(root.root());
}

// --- (g) Backlight: get con la fórmula, set con clamp 1..100 y fórmula (p*max)/100 ---
#[test]
fn g_backlight_percent_matches_go() {
    let root = fake_intel_laptop("bl");
    let bl = LinuxBacklight::with_root(root.clone()).unwrap();

    assert_eq!(bl.brightness_percent().unwrap(), 50); // (500*100)/1000
    bl.set_brightness_percent(75).unwrap();
    assert_eq!(read(&root, &format!("{BL}/brightness")), "750");
    bl.set_brightness_percent(0).unwrap(); // clamp inferior 1 -> 10
    assert_eq!(read(&root, &format!("{BL}/brightness")), "10");
    bl.set_brightness_percent(120).unwrap(); // clamp superior 100 -> 1000
    assert_eq!(read(&root, &format!("{BL}/brightness")), "1000");

    let _ = fs::remove_dir_all(root.root());
}

// --- (h) Periféricos y tweaks: get/set con los fallbacks de Go ---
#[test]
fn h_peripherals_and_tweaks_match_go() {
    let root = fake_intel_laptop("tweaks");
    let per = LinuxPeripherals::with_root(root.clone());
    let tw = LinuxSystemTweaks::with_root(root.clone());

    // kbd backlight: get = brightness != "0"; set on escribe el max_brightness crudo.
    assert!(!per.kbd_backlight().unwrap());
    per.set_kbd_backlight(true).unwrap();
    assert_eq!(
        read(&root, "sys/class/leds/tpacpi::kbd_backlight/brightness"),
        "2"
    );
    assert!(per.kbd_backlight().unwrap());
    per.set_kbd_backlight(false).unwrap();
    assert_eq!(
        read(&root, "sys/class/leds/tpacpi::kbd_backlight/brightness"),
        "0"
    );

    // audio: "0" = off; set on escribe power_save=1 y controller=Y.
    assert!(!tw.audio_power_save().unwrap());
    tw.set_audio_power_save(true).unwrap();
    assert_eq!(
        read(&root, "sys/module/snd_hda_intel/parameters/power_save"),
        "1"
    );
    assert_eq!(
        read(
            &root,
            "sys/module/snd_hda_intel/parameters/power_save_controller"
        ),
        "Y"
    );
    assert!(tw.audio_power_save().unwrap());
    tw.set_audio_power_save(false).unwrap();
    assert_eq!(
        read(&root, "sys/module/snd_hda_intel/parameters/power_save"),
        "0"
    );
    assert_eq!(
        read(
            &root,
            "sys/module/snd_hda_intel/parameters/power_save_controller"
        ),
        "N"
    );

    // autosuspend: get = algún device "auto"; set false = "on" en USB y PCI.
    assert!(!tw.autosuspend().unwrap());
    tw.set_autosuspend(true).unwrap();
    assert_eq!(
        read(&root, "sys/bus/usb/devices/usb1/power/control"),
        "auto"
    );
    assert_eq!(
        read(&root, "sys/bus/pci/devices/0000:00:14.0/power/control"),
        "auto"
    );
    assert!(tw.autosuspend().unwrap());
    tw.set_autosuspend(false).unwrap();
    assert_eq!(read(&root, "sys/bus/usb/devices/usb1/power/control"), "on");

    // NMI watchdog: "1" = on.
    assert!(tw.nmi_watchdog().unwrap());
    tw.set_nmi_watchdog(false).unwrap();
    assert_eq!(read(&root, "proc/sys/kernel/nmi_watchdog"), "0");

    // VM writeback: la API va en segundos, el archivo en centisecs, clamp 100..6000.
    assert_eq!(tw.vm_writeback_seconds().unwrap(), 5);
    tw.set_vm_writeback_seconds(60).unwrap();
    assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "6000");
    tw.set_vm_writeback_seconds(200).unwrap(); // clamp max
    assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "6000");
    tw.set_vm_writeback_seconds(0).unwrap(); // clamp min
    assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "100");

    // WiFi power save: el set escribe el knob de iwlwifi con Y/N (el get depende de `iw`).
    tw.set_wifi_power_save(true).unwrap();
    assert_eq!(read(&root, "sys/module/iwlwifi/parameters/power_save"), "Y");
    tw.set_wifi_power_save(false).unwrap();
    assert_eq!(read(&root, "sys/module/iwlwifi/parameters/power_save"), "N");

    // ASPM: get extrae lo que está entre corchetes; set escribe el policy tal cual.
    let aspm = LinuxAspm::with_root(root.clone()).unwrap();
    assert_eq!(aspm.aspm_policy().unwrap(), "powersave");
    aspm.set_aspm_policy("performance").unwrap();
    assert_eq!(
        read(&root, "sys/module/pcie_aspm/parameters/policy"),
        "performance"
    );

    let _ = fs::remove_dir_all(root.root());
}

// --- Cableado completo: LinuxBackend::with_root propaga la raíz a TODOS los subsistemas ---
#[test]
fn backend_with_root_reaches_every_subsystem() {
    let root = fake_intel_laptop("backend");
    let backend = wattwarden_platform::linux::LinuxBackend::with_root(root.clone()).unwrap();

    let caps = backend.capabilities();
    assert!(caps.has_battery);
    assert!(caps.has_rapl);
    assert!(caps.has_gpu_control);
    assert!(caps.has_backlight_control);
    assert_eq!(backend.cpu.freq_bounds().unwrap(), (400, 3500));
    assert_eq!(backend.battery.battery_percentage().unwrap(), 72);

    // apply_profile(Extreme) tiene que tocar el sysfs FALSO, no el real.
    backend
        .apply_profile(&wattwarden_core::PowerProfile::Extreme)
        .unwrap();
    assert_eq!(read(&root, &format!("{CPU}/cpu1/online")), "1"); // 2 cores
    assert_eq!(read(&root, &format!("{CPU}/cpu2/online")), "0");
    assert_eq!(
        read(&root, &format!("{CPU}/cpu0/cpufreq/scaling_max_freq")),
        "400000"
    );
    assert_eq!(
        read(&root, &format!("{RAPL}/constraint_0_power_limit_uw")),
        "2000000"
    );
    assert_eq!(read(&root, &format!("{BL}/brightness")), "100"); // 10% de 1000
    assert_eq!(read(&root, "proc/sys/vm/dirty_writeback_centisecs"), "6000");
    assert_eq!(read(&root, "proc/sys/kernel/nmi_watchdog"), "0");

    let _ = fs::remove_dir_all(root.root());
}

// --- Nada de esto puede depender del hardware real: la Pi no tiene batería ni RAPL ---
#[test]
fn empty_root_degrades_with_go_fallbacks_and_never_panics() {
    let dir = std::env::temp_dir().join(format!("ww_parity_empty_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let root = SysfsRoot::new(&dir);

    let backend = wattwarden_platform::linux::LinuxBackend::with_root(root.clone()).unwrap();
    // Fallbacks de Go: freq 400/1600, sin GPU/RAPL/backlight, batería 0.
    assert_eq!(backend.cpu.freq_bounds().unwrap(), (400, 1600));
    assert_eq!(backend.battery.battery_percentage().unwrap(), 0);
    assert!(backend.gpu.is_none());
    assert!(backend.rapl.is_none());
    assert!(backend.backlight.is_none());

    // Los perfiles no pueden panickear ni escribir en el /sys real.
    for profile in [
        wattwarden_core::PowerProfile::Performance,
        wattwarden_core::PowerProfile::Extreme,
        wattwarden_core::PowerProfile::Normal,
    ] {
        backend.apply_profile(&profile).unwrap();
    }

    let _ = fs::remove_dir_all(&dir);
}
