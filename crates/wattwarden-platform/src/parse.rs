//! Pure text parsers for the macOS (`pmset`, `ioreg`, `brightness`,
//! `networksetup`) and Windows (`powershell`, `typeperf`, `netsh`) backends.
//!
//! Every function is a byte-for-byte transcription of the Go reference
//! (`hal/backend_darwin.go`, `hal/backend_windows.go`): the same number scan, the
//! same fallback on failure. Keeping them free of any platform `cfg` is what makes
//! the Go-parity assertions runnable on this Linux host — the command plumbing lives
//! in [`crate::exec`] and is mocked per-platform.

/// Go `parseMacFloat`: the first `label[: ]+(-?[0-9]+(\.[0-9]+)?)` match, else `0`.
fn mac_number_after(out: &str, label: &str) -> f64 {
    let bytes = out.as_bytes();
    let Some(idx) = out.find(label) else {
        return 0.0;
    };
    let mut j = idx + label.len();
    let sep_start = j;
    while j < bytes.len() && (bytes[j] == b':' || bytes[j] == b' ') {
        j += 1;
    }
    // Go's regex requires at least one `:` or space between label and number.
    if j == sep_start {
        return 0.0;
    }
    let num_start = j;
    if j < bytes.len() && bytes[j] == b'-' {
        j += 1;
    }
    let digits_start = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j == digits_start {
        return 0.0;
    }
    if j < bytes.len() && bytes[j] == b'.' {
        let dot = j;
        j += 1;
        let frac_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        // Go's `(\.[0-9]+)?` needs a digit after the dot to participate.
        if j == frac_start {
            j = dot;
        }
    }
    out[num_start..j].parse::<f64>().unwrap_or(0.0)
}

/// Go `GetBatteryPercentage` (DarwinBackend): the digits right before the first
/// `%`, else `100` (desktop Macs have no battery).
pub fn mac_battery_percent(pmset_batt: &str) -> u8 {
    let bytes = pmset_batt.as_bytes();
    if let Some(idx) = bytes.iter().position(|&b| b == b'%') {
        let start = idx.saturating_sub(3);
        let digits: String = bytes[start..idx]
            .iter()
            .filter(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        if let Ok(v) = digits.parse::<u32>() {
            return v.min(100) as u8;
        }
    }
    100
}

/// Go `IsCharging` (DarwinBackend): `AC Power` present and not `discharging`.
/// A failed `pmset` yields `""`, i.e. `false` — the same as Go.
pub fn mac_is_charging(pmset_batt: &str) -> bool {
    pmset_batt.contains("AC Power") && !pmset_batt.contains("discharging")
}

/// Go `GetBatteryTime` (DarwinBackend): the `H:MM remaining` token, else `Charging`
/// when plugged in, else `Calculating...`.
pub fn mac_time_remaining(pmset_batt: &str, charging: bool) -> String {
    let b = pmset_batt.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i < b.len() && b[i] == b':' {
                i += 1;
                let sec = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i > sec && b[i..].starts_with(b" remaining") {
                    return String::from_utf8_lossy(&b[start..i]).into_owned();
                }
            }
        } else {
            i += 1;
        }
    }
    if charging {
        "Charging".to_string()
    } else {
        "Calculating...".to_string()
    }
}

/// Go `GetPowerConsumptionWatts` (DarwinBackend): `|Current| * Voltage / 1e6`,
/// `0.0` when either reading is missing/zero.
pub fn mac_power_watts(ioreg_output: &str) -> f64 {
    let current = mac_number_after(ioreg_output, "\"Current\" =").abs();
    let voltage = mac_number_after(ioreg_output, "\"Voltage\" =");
    if current == 0.0 || voltage == 0.0 {
        return 0.0;
    }
    current * voltage / 1_000_000.0
}

/// Go `GetLCDBrightness` (DarwinBackend): `brightness[ =]+([0-9.]+)` × 100,
/// truncated, else `100`.
pub fn mac_brightness_percent(brightness_list: &str) -> u8 {
    let Some(idx) = brightness_list.find("brightness") else {
        return 100;
    };
    let b = brightness_list.as_bytes();
    let mut j = idx + "brightness".len();
    let sep = j;
    while j < b.len() && (b[j] == b' ' || b[j] == b'=') {
        j += 1;
    }
    if j == sep {
        return 100;
    }
    let start = j;
    while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
        j += 1;
    }
    if j == start {
        return 100;
    }
    match brightness_list[start..j].parse::<f64>() {
        // Go: `int(v * 100)` (truncation towards zero).
        Ok(v) => (v * 100.0).clamp(0.0, 100.0) as u8,
        Err(_) => 100,
    }
}

/// Go `getMacWifiDevice`: the `Device:` right after a `Wi-Fi`/`AirPort` port, else
/// `en0`.
pub fn mac_wifi_device(ports_output: &str) -> String {
    let lines: Vec<&str> = ports_output.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Wi-Fi") || line.contains("AirPort") {
            if let Some(next) = lines.get(i + 1) {
                if next.contains("Device:") {
                    if let Some(dev) = next.split_whitespace().nth(1) {
                        return dev.to_string();
                    }
                }
            }
        }
    }
    "en0".to_string()
}

/// Go `GetBatteryPercentage` (WindowsBackend): the raw percent, or `100` when the
/// value is the `>100` "unknown" sentinel.
pub fn win_battery_percent(percent: u8) -> u8 {
    if percent <= 100 {
        percent
    } else {
        100
    }
}

/// Go `GetBatteryTime` (WindowsBackend). `None` models `getPowerStatus() == nil`.
pub fn win_time_remaining(status: Option<(bool, u32)>) -> String {
    match status {
        None => "Calculating...".to_string(),
        Some((true, _)) => "Charging".to_string(),
        Some((false, 0)) | Some((false, 0xFFFF_FFFF)) => "Calculating...".to_string(),
        Some((false, secs)) => format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

/// Go `GetPowerConsumptionWatts` (WindowsBackend): `Voltage * DischargeRate / 1e6`
/// only while discharging, else `0.0`.
pub fn win_power_watts(powershell_out: &str) -> f64 {
    let fields: Vec<&str> = powershell_out.split_whitespace().collect();
    if fields.len() < 3 || fields[0] != "False" {
        return 0.0;
    }
    match (fields[1].parse::<f64>(), fields[2].parse::<f64>()) {
        (Ok(voltage), Ok(discharge)) => voltage * discharge / 1_000_000.0,
        _ => 0.0,
    }
}

/// Go `getWinLoad`: the `% Processor Time` of the third `typeperf` line, as a
/// `0.0..1.0` fraction (the caller scales it by the CPU count for the shared ladder).
pub fn win_load_fraction(typeperf_out: &str) -> f64 {
    let lines: Vec<&str> = typeperf_out.split('\n').collect();
    let Some(line) = lines.get(2) else {
        return 0.0;
    };
    let Some(field) = line.split(',').nth(1) else {
        return 0.0;
    };
    let value = field.trim_matches(|c| c == '"' || c == ' ');
    value.parse::<f64>().map_or(0.0, |v| v / 100.0)
}

/// Go `GetWifiEnable` (WindowsBackend): no interface reported as `Disabled`.
pub fn win_wifi_enabled(netsh_out: &str) -> bool {
    !netsh_out.contains("Disabled")
}

/// Go `GetLCDBrightness` (WindowsBackend): `Atoi`, else `100`.
pub fn win_brightness(powershell_out: &str) -> u8 {
    powershell_out
        .trim()
        .parse::<i64>()
        .map(|v| v.clamp(0, 100) as u8)
        .unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact fake `pmset -g batt` output used by Go's
    /// `TestDarwinBackendNativeCommands`.
    const GO_PMSET_BATT: &str = "Now drawing from 'Battery Power'\n\
         -InternalBattery-0 (id=1) 82%; discharging; 4:12 remaining; present: true";

    #[test]
    fn mac_battery_percent_reads_the_digits_before_the_sign() {
        assert_eq!(mac_battery_percent(GO_PMSET_BATT), 82);
        assert_eq!(mac_battery_percent("Now drawing from 'AC Power'"), 100);
    }

    #[test]
    fn mac_is_charging_matches_go_including_the_failure_fallback() {
        assert!(!mac_is_charging(GO_PMSET_BATT));
        assert!(mac_is_charging("Now drawing from 'AC Power'"));
        // A failed command yields "": Go returns false, not true.
        assert!(!mac_is_charging(""));
    }

    #[test]
    fn mac_time_remaining_prefers_the_hh_mm_token() {
        assert_eq!(mac_time_remaining(GO_PMSET_BATT, false), "4:12");
        assert_eq!(
            mac_time_remaining("Now drawing from 'AC Power'", true),
            "Charging"
        );
        assert_eq!(mac_time_remaining("No battery", false), "Calculating...");
    }

    #[test]
    fn mac_power_watts_matches_go() {
        let ioreg = "\"Current\" = 1000\n\"Voltage\" = 12000";
        assert_eq!(mac_power_watts(ioreg), 12.0);
        // Missing readings -> 0.0.
        assert_eq!(mac_power_watts("nope"), 0.0);
        assert_eq!(mac_power_watts("\"Current\" = 0\n\"Voltage\" = 12000"), 0.0);
        // Current is taken absolute, like Go's `math.Abs`.
        assert_eq!(
            mac_power_watts("\"Current\" = -1000\n\"Voltage\" = 12000"),
            12.0
        );
    }

    #[test]
    fn mac_brightness_percent_matches_go_formula() {
        assert_eq!(mac_brightness_percent("display 0: brightness 0.75"), 75);
        assert_eq!(mac_brightness_percent("display 0: brightness 1.00"), 100);
        // No `brightness` token -> the Go fallback.
        assert_eq!(mac_brightness_percent("no such output"), 100);
    }

    #[test]
    fn mac_wifi_device_matches_go_including_the_en0_fallback() {
        let ports = "Hardware Port: Wi-Fi\nDevice: en1\nEthernet Address: aa:bb\n";
        assert_eq!(mac_wifi_device(ports), "en1");
        assert_eq!(
            mac_wifi_device("Hardware Port: Ethernet\nDevice: en0\n"),
            "en0"
        );
    }

    #[test]
    fn win_battery_percent_matches_go_sentinel() {
        assert_eq!(win_battery_percent(82), 82);
        assert_eq!(win_battery_percent(100), 100);
        // 0xFF is the Win32 "unknown" sentinel (>100) -> 100.
        assert_eq!(win_battery_percent(255), 100);
    }

    #[test]
    fn win_time_remaining_matches_go() {
        assert_eq!(win_time_remaining(None), "Calculating...");
        assert_eq!(win_time_remaining(Some((true, 1234))), "Charging");
        assert_eq!(win_time_remaining(Some((false, 0))), "Calculating...");
        assert_eq!(
            win_time_remaining(Some((false, 0xFFFF_FFFF))),
            "Calculating..."
        );
        assert_eq!(win_time_remaining(Some((false, 3600 + 12 * 60))), "1h 12m");
    }

    #[test]
    fn win_power_watts_matches_go() {
        assert_eq!(win_power_watts("False 12000 1000"), 12.0);
        // PowerOnline == True (charging) -> 0.0.
        assert_eq!(win_power_watts("True 12000 1000"), 0.0);
        assert_eq!(win_power_watts("False"), 0.0);
    }

    #[test]
    fn win_load_fraction_reads_the_third_line() {
        let typeperf =
            "(PDH-CSV 4.0),\n\"\\\\host\\proc\\% time\",\n\"09/22/2026 12:00:00.000\",\"50.00\"";
        assert_eq!(win_load_fraction(typeperf), 0.5);
        assert_eq!(win_load_fraction("too\nshort"), 0.0);
    }

    #[test]
    fn win_wifi_enabled_and_brightness_match_go() {
        assert!(win_wifi_enabled(
            "Admin State    State\nEnabled        Connected"
        ));
        assert!(!win_wifi_enabled(
            "Admin State    State\nDisabled       Disconnected"
        ));
        assert_eq!(win_brightness("50"), 50);
        assert_eq!(win_brightness("not a number"), 100);
    }
}
