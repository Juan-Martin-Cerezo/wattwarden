//! Pure text parsers for the macOS (`pmset`, `ioreg`, `brightness`, `networksetup`) backend.
//!
//! Keeping them free of any platform `cfg` is what makes the assertions runnable on
//! this Linux host — the command plumbing lives in [`crate::exec`] and is mocked per-platform.

/// Parses the first floating point number following a given label token, else returns `0.0`.
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
    // Requires at least one ':' or space between label and number.
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
        // Needs a digit after the dot to participate.
        if j == frac_start {
            j = dot;
        }
    }
    out[num_start..j].parse::<f64>().unwrap_or(0.0)
}

/// Parses battery percentage from `pmset` output: the digits right before the first
/// `%`, else `100` (desktop devices have no battery).
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

/// Determines charging status: `AC Power` present and not `discharging`.
/// A failed command yields `""`, returning `false`.
pub fn mac_is_charging(pmset_batt: &str) -> bool {
    pmset_batt.contains("AC Power") && !pmset_batt.contains("discharging")
}

/// Parses remaining battery time: the `H:MM remaining` token, else `Charging`
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

/// Computes power consumption in Watts: `|Current| * Voltage / 1e6`,
/// returning `0.0` when either reading is missing/zero.
pub fn mac_power_watts(ioreg_output: &str) -> f64 {
    let current = mac_number_after(ioreg_output, "\"Current\" =").abs();
    let voltage = mac_number_after(ioreg_output, "\"Voltage\" =");
    if current == 0.0 || voltage == 0.0 {
        return 0.0;
    }
    current * voltage / 1_000_000.0
}

/// Parses LCD brightness percentage: `brightness[ =]+([0-9.]+)` × 100,
/// truncated, fallback to `100`.
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
        // Truncation towards zero.
        Ok(v) => (v * 100.0).clamp(0.0, 100.0) as u8,
        Err(_) => 100,
    }
}

/// Resolves the Wi-Fi network device: the `Device:` right after a `Wi-Fi`/`AirPort` port, else `en0`.
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PMSET_BATT: &str = "Now drawing from 'Battery Power'\n\
         -InternalBattery-0 (id=1) 82%; discharging; 4:12 remaining; present: true";

    #[test]
    fn mac_battery_percent_reads_the_digits_before_the_sign() {
        assert_eq!(mac_battery_percent(SAMPLE_PMSET_BATT), 82);
        assert_eq!(mac_battery_percent("Now drawing from 'AC Power'"), 100);
    }

    #[test]
    fn mac_is_charging_evaluates_correctly_including_failure_fallback() {
        assert!(!mac_is_charging(SAMPLE_PMSET_BATT));
        assert!(mac_is_charging("Now drawing from 'AC Power'"));
        // A failed command yields empty string, returning false.
        assert!(!mac_is_charging(""));
    }

    #[test]
    fn mac_time_remaining_prefers_the_hh_mm_token() {
        assert_eq!(mac_time_remaining(SAMPLE_PMSET_BATT, false), "4:12");
        assert_eq!(
            mac_time_remaining("Now drawing from 'AC Power'", true),
            "Charging"
        );
        assert_eq!(mac_time_remaining("No battery", false), "Calculating...");
    }

    #[test]
    fn mac_power_watts_calculates_correctly() {
        let ioreg = "\"Current\" = 1000\n\"Voltage\" = 12000";
        assert_eq!(mac_power_watts(ioreg), 12.0);
        // Missing readings -> 0.0.
        assert_eq!(mac_power_watts("nope"), 0.0);
        assert_eq!(mac_power_watts("\"Current\" = 0\n\"Voltage\" = 12000"), 0.0);
        // Current is taken absolute.
        assert_eq!(
            mac_power_watts("\"Current\" = -1000\n\"Voltage\" = 12000"),
            12.0
        );
    }

    #[test]
    fn mac_brightness_percent_calculates_correctly() {
        assert_eq!(mac_brightness_percent("display 0: brightness 0.75"), 75);
        assert_eq!(mac_brightness_percent("display 0: brightness 1.00"), 100);
        // No brightness token -> fallback to 100.
        assert_eq!(mac_brightness_percent("no such output"), 100);
    }

    #[test]
    fn mac_wifi_device_extracts_interface_with_fallback() {
        let ports = "Hardware Port: Wi-Fi\nDevice: en1\nEthernet Address: aa:bb\n";
        assert_eq!(mac_wifi_device(ports), "en1");
        assert_eq!(
            mac_wifi_device("Hardware Port: Ethernet\nDevice: en0\n"),
            "en0"
        );
    }
}
