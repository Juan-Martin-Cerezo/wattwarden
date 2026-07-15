#!/usr/bin/env python3
import curses
import os
import subprocess
import time
import glob
import json

# --- CONFIG & HELPERS ---
REAL_USER = "juan"
USER_ID = 1000
PID_FILE = "/var/run/auto_extreme_daemon.pid"

def run(cmd):
    try:
        return subprocess.check_output(cmd, shell=True, stderr=subprocess.DEVNULL).decode().strip()
    except:
        return ""

def get_sys_val(path):
    try:
        with open(path, 'r') as f:
            return f.read().strip()
    except:
        return ""

def set_sys_val(path, val):
    try:
        with open(path, 'w') as f:
            f.write(str(val))
    except:
        pass

# --- HARDWARE LOGIC ---

def _set_turbo(enabled):
    val = "0" if enabled else "1"
    if os.path.exists("/sys/devices/system/cpu/intel_pstate/no_turbo"):
        set_sys_val("/sys/devices/system/cpu/intel_pstate/no_turbo", val)
    elif os.path.exists("/sys/devices/system/cpu/cpufreq/boost"):
        val_boost = "1" if enabled else "0"
        set_sys_val("/sys/devices/system/cpu/cpufreq/boost", val_boost)

def _get_turbo():
    if os.path.exists("/sys/devices/system/cpu/intel_pstate/no_turbo"):
        return get_sys_val("/sys/devices/system/cpu/intel_pstate/no_turbo") == "0"
    elif os.path.exists("/sys/devices/system/cpu/cpufreq/boost"):
        return get_sys_val("/sys/devices/system/cpu/cpufreq/boost") == "1"
    return True

def get_num_cpus():
    return len(glob.glob("/sys/devices/system/cpu/cpu[0-9]*"))

def get_cores():
    num_cpus = get_num_cpus()
    cores = 1  # cpu0 is always online
    for i in range(1, num_cpus):
        if get_sys_val(f"/sys/devices/system/cpu/cpu{i}/online") == '1':
            cores += 1
    return cores

def set_cores(n):
    num_cpus = get_num_cpus()
    n = max(1, min(num_cpus, n))
    for i in range(1, num_cpus):
        path = f"/sys/devices/system/cpu/cpu{i}/online"
        if os.path.exists(path):
            val = '1' if i < n else '0'
            set_sys_val(path, val)
        
    mhz = get_freq_limit()
    if mhz > 0:
        set_freq_limit(mhz)

def get_cpu_freq_bounds():
    min_val = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq")
    max_val = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
    min_mhz = int(min_val) // 1000 if min_val else 400
    max_mhz = int(max_val) // 1000 if max_val else 1600
    return min_mhz, max_mhz

def get_freq_limit():
    val = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
    return int(val) // 1000 if val else 0

def set_freq_limit(mhz):
    min_mhz, max_mhz = get_cpu_freq_bounds()
    mhz = max(min_mhz, min(max_mhz, mhz))
    max_khz = mhz * 1000
    min_khz = min_mhz * 1000
    for base in glob.glob("/sys/devices/system/cpu/cpu*/cpufreq"):
        set_sys_val(os.path.join(base, "scaling_min_freq"), min_khz)
        set_sys_val(os.path.join(base, "scaling_max_freq"), max_khz)

def _set_audio_powersave(enabled):
    val = "1" if enabled else "0"
    for i in glob.glob("/sys/module/snd_*/parameters/power_save"):
        if os.path.isfile(i):
            set_sys_val(i, val)

def _get_audio_powersave():
    import glob
    paths = glob.glob("/sys/module/snd_*/parameters/power_save")
    if paths:
        return get_sys_val(paths[0]) == "1"
    return False

def get_gpu_path():
    for card in ["card1", "card0"]:
        path = f"/sys/class/drm/{card}/gt_max_freq_mhz"
        if os.path.exists(path):
            return f"/sys/class/drm/{card}"
    return ""

def get_gpu_bounds():
    path = get_gpu_path()
    if not path:
        return 300, 1100
    min_val = get_sys_val(f"{path}/gt_RPn_freq_mhz") or get_sys_val(f"{path}/gt_min_freq_mhz")
    max_val = get_sys_val(f"{path}/gt_RP0_freq_mhz") or get_sys_val(f"{path}/gt_max_freq_mhz")
    min_mhz = int(min_val) if min_val else 300
    max_mhz = int(max_val) if max_val else 1100
    return min_mhz, max_mhz

def get_gpu_limit():
    path = get_gpu_path()
    if not path: return 0
    val = get_sys_val(f"{path}/gt_max_freq_mhz")
    return int(val) if val else 0

def set_gpu_limit(mhz):
    path = get_gpu_path()
    if not path: return
    min_mhz, max_mhz = get_gpu_bounds()
    mhz = max(min_mhz, min(max_mhz, mhz))
    set_sys_val(f"{path}/gt_max_freq_mhz", mhz)

def get_temp():
    out = run("sensors | grep 'Package id 0'")
    return out.split("+")[1].split(".")[0] if "+" in out else "??"

def get_power():
    try:
        c_path = "/sys/class/power_supply/BAT0/current_now"
        v_path = "/sys/class/power_supply/BAT0/voltage_now"
        if not os.path.exists(c_path): return 0.0
        c = int(get_sys_val(c_path))
        v = int(get_sys_val(v_path))
        return (c * v) / 10**12
    except: return 0.0

def get_battery():
    return get_sys_val("/sys/class/power_supply/BAT0/capacity")

# Intel RAPL & EPP Helpers
def _get_rapl_path(type_name="long_term"):
    import glob
    for p in glob.glob("/sys/class/powercap/intel-rapl:[0-9]/constraint_*_name"):
        val = get_sys_val(p)
        if type_name in val:
            return p.replace("_name", "_power_limit_uw")
    return ""

def get_rapl_bounds():
    max_uw = 115000000
    min_uw = 5000000
    base = "/sys/class/powercap/intel-rapl:0"
    if os.path.exists(f"{base}/max_power_range_uw"):
        val = get_sys_val(f"{base}/max_power_range_uw")
        if val: max_uw = int(val)
    if os.path.exists(f"{base}/min_power_range_uw"):
        val = get_sys_val(f"{base}/min_power_range_uw")
        if val: min_uw = int(val)
    return min_uw // 1000000, max_uw // 1000000

def get_rapl_pl1():
    path = _get_rapl_path("long_term")
    val = get_sys_val(path) if path else ""
    return int(val) // 1000000 if val else 0

def set_rapl_pl1(watts):
    path = _get_rapl_path("long_term")
    if path:
        min_w, max_w = get_rapl_bounds()
        set_sys_val(path, max(min_w, min(max_w, watts)) * 1000000)

def get_rapl_pl2():
    path = _get_rapl_path("short_term")
    val = get_sys_val(path) if path else ""
    return int(val) // 1000000 if val else 0

def set_rapl_pl2(watts):
    path = _get_rapl_path("short_term")
    if path:
        min_w, max_w = get_rapl_bounds()
        set_sys_val(path, max(min_w, min(max_w, watts)) * 1000000)

EPP_PREFERENCES = ["default", "performance", "balance_performance", "balance_power", "power"]

def get_epp():
    val = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
    if val in EPP_PREFERENCES:
        return val
    return "default"

def set_epp(pref):
    if pref in EPP_PREFERENCES:
        for i in glob.glob("/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference"):
            if os.path.isfile(i):
                set_sys_val(i, pref)

def set_governor(gov):
    for i in glob.glob("/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor"):
        if os.path.isfile(i):
            set_sys_val(i, gov)

ASPM_POLICIES = ["default", "performance", "powersave", "powersupersave"]

def get_aspm_policy():
    val = get_sys_val("/sys/module/pcie_aspm/parameters/policy")
    for p in ASPM_POLICIES:
        if f"[{p}]" in val:
            return p
    return "default"

def set_aspm_policy(policy):
    if policy in ASPM_POLICIES:
        set_sys_val("/sys/module/pcie_aspm/parameters/policy", policy)

# Hyprland Signature and Control Helpers
def is_hyprland():
    return run("pgrep -x Hyprland") != ""

def run_hyprctl(cmd_args):
    if not is_hyprland(): return ""
    user = os.environ.get("SUDO_USER", "")
    if user and user != "root":
        return run(f"su - {user} -c 'hyprctl {cmd_args}'")
    return run(f"hyprctl {cmd_args}")

def get_hypr_animations():
    out = run_hyprctl("getoption animations:enabled -j")
    try:
        data = json.loads(out)
        return data.get("bool", False) or data.get("int", 0) == 1
    except:
        return False

def _get_main_monitor():
    try:
        out = run_hyprctl("monitors -j")
        data = json.loads(out)
        if data and isinstance(data, list):
            return data[0]["name"]
    except:
        pass
    return "eDP-1"

def get_monitor_refresh_rates():
    rates = []
    if is_hyprland():
        try:
            out = run_hyprctl("monitors -j")
            data = json.loads(out)
            for m in data:
                if m.get("focused", False) or m.get("id", 0) == 0:
                    for mode in m.get("availableModes", []):
                        if "@" in mode:
                            rates.append(float(mode.split("@")[1][:5]))
                    break
        except: pass
    if not rates:
        try:
            out = run("xrandr")
            for line in out.splitlines():
                if "*" in line or "+" in line:
                    parts = line.split()
                    for p in parts[1:]:
                        if p.replace('.', '').replace('*','').replace('+','').isdigit():
                            rates.append(float(p.replace('*','').replace('+','')))
        except: pass
    if not rates:
        rates = [60.0]
    return rates

def set_refresh_rate(target):
    rates = get_monitor_refresh_rates()
    fps = max(rates) if target == "max" else min(rates) if target == "min" else target
    if is_hyprland():
        mon = _get_main_monitor()
        res = "1920x1080"
        try:
            out = run_hyprctl("monitors -j")
            data = json.loads(out)
            for m in data:
                if m.get("name") == mon:
                    res = f"{m.get('width', 1920)}x{m.get('height', 1080)}"
        except: pass
        run_hyprctl(f'eval \'hl.monitor({{ output = "{mon}", mode = "{res}@{fps}", position = "auto", scale = 1 }})\'')
    else:
        out = run("xrandr | grep ' connected'")
        if out:
            mon = out.split()[0]
            run(f"xrandr --output {mon} --rate {fps}")

def set_hypr_effects(enabled):
    val = "true" if enabled else "false"
    lua = f"hl.config({{ animations = {{ enabled = {val} }}, decoration = {{ blur = {{ enabled = {val} }}, shadow = {{ enabled = {val} }} }} }})"
    run_hyprctl(f"eval '{lua}'")

# Helpers
def _set_brightness_target(target):
    max_b = int(run("brightnessctl m") or 100)
    if target == 'max':
        val = max_b
    elif target == 'min':
        val = 1
    else:
        val = int(max_b * target / 100)
    subprocess.run(f"brightnessctl s {val}", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

# WiFi Interface and Power Save Helpers
def get_wifi_interface():
    paths = glob.glob("/sys/class/net/wl*")
    if paths:
        return os.path.basename(paths[0])
    return ""

def get_wifi_powersave():
    iface = get_wifi_interface()
    if not iface: return False
    out = run(f"iw dev {iface} get power_save")
    return "Power save: on" in out

def set_wifi_powersave(s):
    iface = get_wifi_interface()
    if not iface: return
    val = "on" if s else "off"
    subprocess.run(f"iw dev {iface} set power_save {val}", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

# Keyboard Backlight Helpers
def _get_kbd_path():
    import glob
    paths = glob.glob("/sys/class/leds/*kbd_backlight*/brightness")
    return paths[0] if paths else ""

def get_kbd_backlight():
    path = _get_kbd_path()
    if not os.path.exists(path): return False
    return get_sys_val(path) != "0"

def set_kbd_backlight(s):
    path = _get_kbd_path()
    if not os.path.exists(path): return
    max_path = path.replace("brightness", "max_brightness") if path else ""
    max_val = get_sys_val(max_path) or "1"
    set_sys_val(path, max_val if s else "0")

# --- PROCESS MONITORING BACKEND ---
def get_top_power_processes(cpu_w, screen_w, total_w):
    output = run("ps -eo pid,%cpu,%mem,comm --sort=-%cpu | head -n 12")
    processes = []
    lines = output.split("\n")[1:]
    
    for line in lines:
        parts = line.split()
        if len(parts) >= 4:
            pid = parts[0]
            cpu_pct = float(parts[1])
            mem_pct = float(parts[2])
            name = " ".join(parts[3:])
            
            num_cpus = get_num_cpus()
            active_cores = get_cores()
            
            total_possible_cpu = active_cores * 100.0
            cpu_ratio = cpu_pct / total_possible_cpu if total_possible_cpu > 0 else 0.0
            
            proc_cpu_w = cpu_ratio * cpu_w
            proc_screen_w = 0.0
            low_name = name.lower()
            if "hyprland" in low_name or "qs" in low_name or "quickshell" in low_name:
                proc_screen_w = screen_w * 0.4
            elif "kitty" in low_name or "foot" in low_name or "alacritty" in low_name:
                proc_screen_w = screen_w * 0.2
            elif "chrome" in low_name or "brave" in low_name or "firefox" in low_name or "zen" in low_name:
                proc_screen_w = screen_w * 0.3
            
            proc_mem_w = (mem_pct / 100.0) * (total_w * 0.1)
            estimated_w = proc_cpu_w + proc_screen_w + proc_mem_w + 0.01
            
            if estimated_w > total_w:
                estimated_w = total_w * (cpu_pct / 100.0)
            
            processes.append({
                "pid": pid,
                "cpu": cpu_pct,
                "mem": mem_pct,
                "name": name,
                "power": estimated_w
            })
    
    processes.sort(key=lambda x: x["power"], reverse=True)
    return processes

def get_current_mode():
    if os.path.exists(PID_FILE):
        return "⚡ AUTO EXTREME (Dynamic)"
    
    epp = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
    no_turbo = "0" if _get_turbo() else "1"
    
    num_cpus = get_num_cpus()
    active_cores = get_cores()
            
    if epp == "performance" and no_turbo == "0" and active_cores == num_cpus:
        return "⚡ PERFORMANCE (Maximum)"
    elif epp == "power" and active_cores == 1:
        return "🔋 EXTREME (Fixed Minimum)"
    elif epp == "balance_performance" or epp == "default":
        return "♻  RESTORE (Balanced)"
    else:
        return "⚙  CUSTOM / MANUAL"

# --- GRAPHS DRAWING ENGINE ---
class PowerDashboard:
    def __init__(self):
        self.history = []
        self.last_rapl_energy = 0
        self.last_rapl_time = 0.0

    def get_power_stats(self):
        c_now_val = get_sys_val("/sys/class/power_supply/BAT0/current_now")
        v_now_val = get_sys_val("/sys/class/power_supply/BAT0/voltage_now")
        cap_val = get_sys_val("/sys/class/power_supply/BAT0/capacity")
        charge_now_val = get_sys_val("/sys/class/power_supply/BAT0/charge_now")
        status = get_sys_val("/sys/class/power_supply/BAT0/status") or "Discharging"
        
        current = float(c_now_val) / 10**6 if c_now_val else 0.0
        voltage = float(v_now_val) / 10**6 if v_now_val else 0.0
        charge_now = float(charge_now_val) / 10**6 if charge_now_val else 0.0
        capacity = int(cap_val) if cap_val else 0
        
        total_w = current * voltage
        
        time_str = "N/A"
        if status.lower() == "discharging" and current > 0.01:
            hours = charge_now / current
            h = int(hours)
            m = int((hours - h) * 60)
            time_str = f"{h}h {m}m"
        elif status.lower() == "charging":
            time_str = "Charging..."
            charge_full_val = get_sys_val("/sys/class/power_supply/BAT0/charge_full")
            if charge_full_val and current > 0.01:
                full = float(charge_full_val) / 10**6
                missing = full - charge_now
                if missing > 0:
                    hours = missing / current
                    h = int(hours)
                    m = int((hours - h) * 60)
                    time_str = f"Charging ({h}h {m}m)"
        elif status.lower() == "full":
            time_str = "Full"

        cpu_w = 0.0
        rapl_path = _get_rapl_path("long_term")
        uj_val = get_sys_val(rapl_path.replace("_power_limit_uw", "_energy_uj")) if rapl_path else ""
        now_time = time.time()
        if uj_val:
            uj = int(uj_val)
            if self.last_rapl_energy > 0 and now_time > self.last_rapl_time:
                diff_j = (uj - self.last_rapl_energy) / 10**6
                diff_t = now_time - self.last_rapl_time
                if diff_j >= 0 and diff_t > 0:
                    cpu_w = diff_j / diff_t
            self.last_rapl_energy = uj
            self.last_rapl_time = now_time
        else:
            load = float(run("cat /proc/loadavg | awk '{print $1}'") or 0.5)
            active_cores = get_cores()
            cpu_w = 1.5 + (load * 0.8 * active_cores)
            if cpu_w > total_w and total_w > 0.1:
                cpu_w = total_w * 0.4
                
        bl_paths = glob.glob("/sys/class/backlight/*")
        bl_path = bl_paths[0] if bl_paths else "/sys/class/backlight/intel_backlight"
        max_b = float(get_sys_val(f"{bl_path}/max_brightness") or 100)
        curr_b = float(get_sys_val(f"{bl_path}/brightness") or 10)
        pct_b = curr_b / max_b if max_b > 0 else 0.1
        screen_w = 0.8 + (pct_b * 3.2)

        others_w = total_w - cpu_w - screen_w
        if others_w < 0:
            others_w = 0.5
            total_w = cpu_w + screen_w + others_w

        return {
            "total_w": total_w,
            "cpu_w": cpu_w,
            "screen_w": screen_w,
            "others_w": others_w,
            "status": status,
            "capacity": capacity,
            "time_str": time_str,
            "voltage": voltage,
            "current": current
        }

    def draw_graph(self, stdscr, start_y, start_x, height, width, data_list, max_val_label=15.0):
        for y in range(height):
            stdscr.addstr(start_y + y, start_x, " " * width)
            
        if not data_list:
            return

        max_in_list = max(data_list)
        graph_max = max(max_val_label, max_in_list * 1.1)
        
        available_cols = width - 15
        needed_points = available_cols * 2
        
        if len(data_list) < needed_points:
            visible_data = [0.0] * (needed_points - len(data_list)) + data_list
        else:
            visible_data = data_list[-needed_points:]
            
        sub_y_total = height * 4
        heights = [int((val / graph_max) * (sub_y_total - 1)) if graph_max > 0 else 0 for val in visible_data]
        
        for x_idx in range(available_cols):
            x_pos = start_x + x_idx
            d1 = x_idx * 2
            d2 = d1 + 1
            h1 = heights[d1]
            h2 = heights[d2]
            
            val_avg = (visible_data[d1] + visible_data[d2]) / 2.0
            if val_avg < 6.0:
                color = curses.color_pair(2) # Green
            elif val_avg < 12.0:
                color = curses.color_pair(3) # Yellow
            else:
                color = curses.color_pair(4) # Red
            
            for r in range(height):
                screen_y = start_y + height - 1 - r
                row_bottom = r * 4
                dots = 0
                
                for dot_y in range(4):
                    pixel_height = row_bottom + dot_y
                    if h1 >= pixel_height:
                        if dot_y == 0: dots |= 0x1
                        elif dot_y == 1: dots |= 0x2
                        elif dot_y == 2: dots |= 0x4
                        elif dot_y == 3: dots |= 0x40
                        
                for dot_y in range(4):
                    pixel_height = row_bottom + dot_y
                    if h2 >= pixel_height:
                        if dot_y == 0: dots |= 0x8
                        elif dot_y == 1: dots |= 0x10
                        elif dot_y == 2: dots |= 0x20
                        elif dot_y == 3: dots |= 0x80
                
                if dots > 0:
                    char_code = 0x2800 + dots
                    stdscr.addstr(screen_y, x_pos, chr(char_code), color)
                else:
                    stdscr.addstr(screen_y, x_pos, " ")

        step_val = graph_max / (height - 1) if height > 1 else 1.0
        for y_offset in range(height):
            screen_y = start_y + height - 1 - y_offset
            val_label = y_offset * step_val
            
            if val_label < 6.0:
                l_color = curses.color_pair(2)
            elif val_label < 12.0:
                l_color = curses.color_pair(3)
            else:
                l_color = curses.color_pair(4)
                
            stdscr.addstr(screen_y, start_x + width - 12, "│ ", curses.A_DIM)
            stdscr.addstr(screen_y, start_x + width - 10, f"{val_label:4.1f} W", l_color)

# --- PANEL CONTROLS ---

# --- POWER MODES & DAEMON LOGIC ---
DEFAULTS_FILE = "/tmp/power_center_defaults.json"

def save_defaults_if_needed():
    if not os.path.exists(DEFAULTS_FILE):
        defaults = {
            "cores": get_cores(),
            "freq_limit": get_freq_limit(),
            "gpu_limit": get_gpu_limit(),
            "rapl_pl1": get_rapl_pl1(),
            "rapl_pl2": get_rapl_pl2(),
            "epp": get_epp(),
            "aspm": get_aspm_policy(),
            "turbo": _get_turbo(),
            "refresh_rate": max(get_monitor_refresh_rates()),
            "brightness": 50
        }
        try:
            if is_hyprland():
                out = run_hyprctl("monitors -j")
                data = json.loads(out)
                defaults["refresh_rate"] = data[0].get("refreshRate", 60.0)
        except: pass
        
        try:
            with open(DEFAULTS_FILE, "w") as f:
                json.dump(defaults, f)
        except: pass

def get_default_value(key, fallback):
    if os.path.exists(DEFAULTS_FILE):
        try:
            with open(DEFAULTS_FILE, "r") as f:
                data = json.load(f)
                return data.get(key, fallback)
        except: pass
    return fallback

def stop_daemon():
    if os.path.exists(PID_FILE):
        try:
            with open(PID_FILE, 'r') as f:
                pid = int(f.read().strip())
            os.kill(pid, 15)
        except:
            pass
        os.remove(PID_FILE)

def apply_mode_performance():
    save_defaults_if_needed()
    stop_daemon()
    set_refresh_rate("max")
    set_cores(get_num_cpus())
    min_cpu, max_cpu = get_cpu_freq_bounds()
    set_freq_limit(max_cpu)
    min_w, max_w = get_rapl_bounds()
    set_rapl_pl1(max_w)
    set_rapl_pl2(max_w)
    set_epp("performance")
    set_aspm_policy("performance")
    _set_turbo(True)
    min_gpu, max_gpu = get_gpu_bounds()
    set_gpu_limit(max_gpu)
    _set_brightness_target("max")
    set_kbd_backlight(True)
    run("rfkill unblock wifi")
    set_wifi_powersave(False)
    run("rfkill unblock bluetooth")
    _set_audio_powersave(False)
    for p in __import__("glob").glob("/sys/bus/pci/devices/*/power/control"): set_sys_val(p, "on")
    for p in __import__("glob").glob("/sys/bus/usb/devices/*/power/control"): set_sys_val(p, "on")
    set_sys_val("/proc/sys/kernel/nmi_watchdog", "1")
    set_sys_val("/proc/sys/vm/dirty_writeback_centisecs", 500)
    set_sys_val("/proc/sys/vm/dirty_expire_centisecs", 500)

def apply_mode_extreme():
    import glob
    save_defaults_if_needed()
    stop_daemon()
    set_refresh_rate("min")
    set_cores(1)
    set_governor("powersave")
    min_gpu, max_gpu = get_gpu_bounds()
    set_gpu_limit(min_gpu)
    min_w, max_w = get_rapl_bounds()
    set_rapl_pl1(min_w)
    set_rapl_pl2(min_w)
    set_epp("power")
    _set_turbo(False)
    min_cpu, max_cpu = get_cpu_freq_bounds()
    set_freq_limit(min_cpu)
    _set_brightness_target("min")
    set_kbd_backlight(False)
    set_wifi_powersave(True)
    subprocess.run("rfkill block bluetooth", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _set_audio_powersave(True)
    set_sys_val("/proc/sys/kernel/nmi_watchdog", "0")
    set_aspm_policy("powersupersave")
    for p in glob.glob("/sys/bus/pci/devices/*/power/control"): set_sys_val(p, "auto")
    for p in glob.glob("/sys/bus/usb/devices/*/power/control"): set_sys_val(p, "auto")
    set_sys_val("/proc/sys/vm/dirty_writeback_centisecs", "15000")
    set_sys_val("/proc/sys/vm/dirty_expire_centisecs", "15000")
    subprocess.Popen("systemctl stop tlp auto-cpufreq power-profiles-daemon thermald system76-power >/dev/null 2>&1; killall -q tlp auto-cpufreq thermald system76-power >/dev/null 2>&1", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def apply_mode_restore():
    stop_daemon()
    set_refresh_rate(get_default_value("refresh_rate", 60.0))
    
    set_cores(get_default_value("cores", get_num_cpus()))
    set_governor("powersave")
    set_epp(get_default_value("epp", "balance_performance"))
    _set_turbo(get_default_value("turbo", True))
    set_freq_limit(get_default_value("freq_limit", 9999)) 
    set_gpu_limit(get_default_value("gpu_limit", 9999))
    
    set_rapl_pl1(get_default_value("rapl_pl1", 28)) 
    set_rapl_pl2(get_default_value("rapl_pl2", 28))
    
    _set_brightness_target(get_default_value("brightness", 50))
    set_kbd_backlight(True)
    
    run("rfkill unblock wifi")
    set_wifi_powersave(True)
    run("rfkill unblock bluetooth")
    
    _set_audio_powersave(True)
    set_aspm_policy(get_default_value("aspm", "default"))
    
    for p in glob.glob("/sys/bus/pci/devices/*/power/control"): set_sys_val(p, "auto")
    for p in glob.glob("/sys/bus/usb/devices/*/power/control"): set_sys_val(p, "auto")
    
    set_sys_val("/proc/sys/kernel/nmi_watchdog", "1")
    set_sys_val("/proc/sys/vm/dirty_writeback_centisecs", "500")
    set_sys_val("/proc/sys/vm/dirty_expire_centisecs", "3000")

def apply_mode_autoextreme():
    stop_daemon()
    subprocess.run(f"python3 {os.path.abspath(__file__)} daemon &", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def daemon_loop():
    with open(PID_FILE, 'w') as f:
        f.write(str(os.getpid()))
        
    # Start by initializing the system to absolute dynamic minimums
    min_cpu, hw_max_cpu = get_cpu_freq_bounds()
    max_cpu = int(min_cpu + (hw_max_cpu - min_cpu) * 0.4)
    
    min_gpu, hw_max_gpu = get_gpu_bounds()
    max_gpu = int(min_gpu + (hw_max_gpu - min_gpu) * 0.4)
    
    min_w, hw_max_w = get_rapl_bounds()
    max_w = int(min_w + (hw_max_w - min_w) * 0.4)
    
    max_cores = max(1, get_num_cpus() // 2)

    set_cores(1)
    set_freq_limit(min_cpu)
    set_gpu_limit(min_gpu)
    set_rapl_pl1(min_w)
    set_rapl_pl2(min_w)
    set_epp("power")
    _set_turbo(False)
    set_aspm_policy("powersupersave")
    set_wifi_powersave(True)

    power_level = 0.0

    last_cores = -1
    last_cpu = -1
    last_gpu = -1
    last_rapl = -1
    last_epp = ""
    last_turbo = -1
    last_brightness = -1

    while True:
        try:
            time.sleep(10)
            
            try:
                with open('/proc/loadavg', 'r') as f:
                    load = float(f.read().split()[0])
            except:
                load = 0.0
                
            # Map the 1-minute load average to a 0.0 - 1.0 scale based on total cores
            power_level = min(1.0, load / get_num_cpus())
            
            # Quantize to just 4 giant steps: 0%, 33%, 66%, 100%
            # This makes the system incredibly resistant to changes.
            discrete_power = round(power_level * 3) / 3.0
            
            target_cores = max(1, int(1 + discrete_power * (max_cores - 1)))
            target_cpu = int(min_cpu + discrete_power * (max_cpu - min_cpu))
            target_gpu = int(min_gpu + discrete_power * (max_gpu - min_gpu))
            target_rapl = int(min_w + discrete_power * (max_w - min_w))
            
            # Focus check only once every 10 seconds to save CPU
            is_heavy_ui = False
            if is_hyprland():
                try:
                    out = run_hyprctl("activewindow -j")
                    win_class = json.loads(out).get("class", "").lower()
                    is_heavy_ui = any(x in win_class for x in ["chrome", "firefox", "brave", "zen", "code", "idea", "studio", "cursor"])
                except: pass
            max_b = 30 if is_heavy_ui else 15
            min_b = 10
            target_brightness = max(min_b, min(max_b, int(min_b + discrete_power * (max_b - min_b))))
            
            # EPP should always be power in Auto Extreme
            target_epp = "power"
                
            target_turbo = discrete_power >= 0.8
            
            if target_cores != last_cores:
                set_cores(target_cores)
                last_cores = target_cores
                
            if target_cpu != last_cpu:
                set_freq_limit(target_cpu)
                last_cpu = target_cpu
                
            if target_gpu != last_gpu:
                set_gpu_limit(target_gpu)
                last_gpu = target_gpu
                
            if target_rapl != last_rapl:
                set_rapl_pl1(target_rapl)
                set_rapl_pl2(target_rapl)
                last_rapl = target_rapl
                
            if target_epp != last_epp:
                set_epp(target_epp)
                last_epp = target_epp
                
            if target_turbo != last_turbo:
                _set_turbo(target_turbo)
                last_turbo = target_turbo
                
            if target_brightness != last_brightness:
                _set_brightness_target(target_brightness)
                last_brightness = target_brightness
            
        except Exception as e:
            time.sleep(10)

OPTIONS = [
    {
        "category": "CPU & COMPUTE",
        "name": "Active Cores",
        "type": "value",
        "get": lambda: get_cores(),
        "set": lambda v, d: set_cores(v + d),
        "desc": "Limiting cores saves a lot of battery but reduces multi-tasking performance.",
        "safety": "SAFE"
    },
    {
        "category": "CPU & COMPUTE",
        "name": "CPU Freq (MHz)",
        "type": "value",
        "get": lambda: get_freq_limit(),
        "set": lambda v, d: set_freq_limit(v + (d * 100)),
        "desc": "Maximum processor frequency. Reducing this saves a massive amount of energy.",
        "safety": "SAFE"
    },
    {
        "category": "CPU & COMPUTE",
        "name": "Energy Perf Pref",
        "type": "value",
        "get": lambda: get_epp(),
        "set": lambda v, d: set_epp(EPP_PREFERENCES[(EPP_PREFERENCES.index(v) + d) % len(EPP_PREFERENCES)]),
        "desc": "Energy Performance Preference (EPP). 'power' forces the processor to be as efficient as possible.",
        "safety": "SAFE"
    },
    {
        "category": "POWER LIMITS",
        "name": "RAPL PL1 (W)",
        "type": "value",
        "get": lambda: get_rapl_pl1(),
        "set": lambda v, d: set_rapl_pl1(v + d),
        "desc": "Sustained power limit (PL1).",
        "safety": "SAFE"
    },
    {
        "category": "POWER LIMITS",
        "name": "RAPL PL2 (W)",
        "type": "value",
        "get": lambda: get_rapl_pl2(),
        "set": lambda v, d: set_rapl_pl2(v + d),
        "desc": "Turbo power limit (PL2). Restricting it prevents massive heat spikes.",
        "safety": "SAFE"
    },
    {
        "category": "POWER LIMITS",
        "name": "PCIe ASPM Policy",
        "type": "value",
        "get": lambda: get_aspm_policy(),
        "set": lambda v, d: set_aspm_policy(ASPM_POLICIES[(ASPM_POLICIES.index(v) + d) % len(ASPM_POLICIES)]),
        "desc": "PCIe Active State Power Management. 'powersupersave' saves more energy.",
        "safety": "SAFE"
    },
    {
        "category": "CPU & COMPUTE",
        "name": "Turbo Boost",
        "type": "toggle",
        "get": lambda: _get_turbo(),
        "set": lambda s: _set_turbo(s),
        "desc": "Disabling Turbo prevents heat spikes and massive instant consumption.",
        "safety": "WARN"
    },
    {
        "category": "GPU & DISPLAY",
        "name": "Freq iGPU (MHz)",
        "type": "value",
        "get": lambda: get_gpu_limit(),
        "set": lambda v, d: set_gpu_limit(v + (d * 50)),
        "desc": "Integrated graphics limit. Reducing it saves energy.",
        "safety": "SAFE"
    },
    {
        "category": "GPU & DISPLAY",
        "name": "LCD Brightness (%)",
        "type": "value",
        "get": lambda: int((int(run("brightnessctl g"))/int(run("brightnessctl m")))*100) if run("brightnessctl m") else 0,
        "set": lambda v, d: subprocess.run(f"brightnessctl s {5 if d > 0 else -5}%", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL),
        "desc": "The panel is the biggest consumer after the CPU. Keep below 10%.",
        "safety": "SAFE"
    },
    {
        "category": "RADIOS & PERIPHERALS",
        "name": "Keyboard Light",
        "type": "toggle",
        "get": lambda: get_kbd_backlight(),
        "set": lambda s: set_kbd_backlight(s),
        "desc": "Turning off keyboard backlight saves a tiny fraction of a watt.",
        "safety": "SAFE"
    },
    {
        "category": "HARDWARE & RADIOS",
        "name": "Bluetooth",
        "type": "toggle",
        "get": lambda: "Soft blocked: no" in run("rfkill list bluetooth"),
        "set": lambda s: subprocess.run(f"rfkill {'unblock' if s else 'block'} bluetooth", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL),
        "desc": "Bluetooth radio cut off. Prevents the chip from scanning for devices.",
        "safety": "SAFE"
    },
    {
        "category": "HARDWARE & RADIOS",
        "name": "WiFi Enable",
        "type": "toggle",
        "get": lambda: "Soft blocked: no" in run("rfkill list wifi"),
        "set": lambda s: subprocess.run(f"rfkill {'unblock' if s else 'block'} wifi", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL),
        "desc": "Toggle WiFi radio (rfkill). Disable to save constant radio polling energy.",
        "safety": "SAFE"
    },
    {
        "category": "HARDWARE & RADIOS",
        "name": "WiFi Power Save",
        "type": "toggle",
        "get": lambda: get_wifi_powersave(),
        "set": lambda s: set_wifi_powersave(s),
        "desc": "Enables WiFi card power saving mode. Reduces consumption but may increase latency.",
        "safety": "SAFE"
    },
    {
        "category": "HARDWARE & RADIOS",
        "name": "Audio Power Save",
        "type": "toggle",
        "get": lambda: _get_audio_powersave(),
        "set": lambda s: _set_audio_powersave(s),
        "desc": "Suspends the audio chip after 1s of inactivity. Prevents static 'pop'.",
        "safety": "SAFE"
    },
    {
        "category": "SYSTEM ACTIONS",
        "name": "Autosuspend PCI/USB",
        "type": "toggle",
        "get": lambda: "auto" in run("cat /sys/bus/pci/devices/*/power/control 2>/dev/null | head -n 1"),
        "set": lambda s: [set_sys_val(p, "auto" if s else "on") for p in glob.glob("/sys/bus/pci/devices/*/power/control")] + [set_sys_val(p, "auto" if s else "on") for p in glob.glob("/sys/bus/usb/devices/*/power/control")],
        "desc": "Suspends inactive USB ports and PCIe lines. May affect peripherals.",
        "safety": "DANGER"
    },
    {
        "category": "SYSTEM ACTIONS",
        "name": "Watchdog Kernel",
        "type": "toggle",
        "get": lambda: get_sys_val("/proc/sys/kernel/nmi_watchdog") == "1",
        "set": lambda s: set_sys_val("/proc/sys/kernel/nmi_watchdog", "1" if s else "0"),
        "desc": "Disable kernel safety interrupts. Reduces CPU wakeups.",
        "safety": "WARN"
    },
    {
        "category": "SYSTEM ACTIONS",
        "name": "VM Writeback (s)",
        "type": "value",
        "get": lambda: int(get_sys_val("/proc/sys/vm/dirty_writeback_centisecs") or 500) // 100,
        "set": lambda v, d: [set_sys_val("/proc/sys/vm/dirty_writeback_centisecs", max(1, v + d) * 100), set_sys_val("/proc/sys/vm/dirty_expire_centisecs", max(1, v + d) * 100)],
        "desc": "Virtual disk writeback interval. High values reduce SSD usage.",
        "safety": "WARN"
    },
    {
        "category": "SYSTEM ACTIONS",
        "name": "Process Purge",
        "type": "action",
        "exec": lambda: subprocess.run("pkill -f 'brave|discord|telegram|code|electron'", shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL),
        "desc": "Closes heavy applications (Brave, Code, Discord, etc.) to free up RAM and CPU.",
        "safety": "DANGER"
    },
    {
        "category": "POWER PROFILES",
        "name": "⚡ PERFORMANCE MODE",
        "type": "action",
        "exec": apply_mode_performance,
        "desc": "TURBO BOOST + all cores + max freq + max GPU. High consumption.",
        "safety": "WARN"
    },
    {
        "category": "POWER PROFILES",
        "name": "🔋 EXTREME MODE",
        "type": "action",
        "exec": apply_mode_extreme,
        "desc": "Absolute minimum hardware bounds. Minimum cores, min freq, min GPU. Maximum battery savings.",
        "safety": "WARN"
    },
    {
        "category": "POWER PROFILES",
        "name": "⚡ AUTO EXTREME MODE",
        "type": "action",
        "exec": apply_mode_autoextreme,
        "desc": "Extreme baseline + dynamic scaling proportional to your hardware bounds based on instant CPU load.",
        "safety": "SAFE"
    },
    {
        "category": "POWER PROFILES",
        "name": "♻  RESTORE MODE",
        "type": "action",
        "exec": apply_mode_restore,
        "desc": "Restores original system defaults captured at the first launch.",
        "safety": "SAFE"
    }
]

def draw_view_menu(stdscr, idx, h, w):
    draw = get_power()
    bat = get_battery()
    temp = get_temp()
    stdscr.addstr(2, 2, "SYSTEM STATUS:", curses.A_BOLD)
    stdscr.addstr(3, 4, f"BATTERY: {bat}%", curses.color_pair(2 if bat and int(bat) > 20 else 4))
    stdscr.addstr(3, 20, f"CONSUMPTION: {draw:.2f}W", curses.color_pair(3))
    stdscr.addstr(3, 40, f"TEMP: {temp}°C", curses.color_pair(4 if temp.isdigit() and int(temp) > 80 else 3))
    
    stdscr.addstr(4, 4, f"CORES: {get_cores()}/{get_num_cpus()} | CPU: {get_freq_limit()}MHz | EPP: {get_epp()} | PL1: {get_rapl_pl1()}W")
    mode = get_current_mode()
    mode_str = f" >>> PROFILE: {mode} <<< "
    x_pos = max(4, w - len(mode_str) - 2)
    try:
        stdscr.addstr(4, x_pos, mode_str, curses.color_pair(6 if "AUTO" in mode else 2) | curses.A_REVERSE | curses.A_BOLD)
    except:
        pass

    stdscr.addstr(6, 2, "HARDWARE CONTROLS (Use Arrows / Enter):", curses.A_BOLD | curses.color_pair(6))
    
    y = 8
    current_cat = None
    
    for i, opt in enumerate(OPTIONS):
        if y >= h - 6: break
            
        cat = opt.get("category", "GENERAL")
        if cat != current_cat:
            current_cat = cat
            stdscr.addstr(y, 2, f" {cat} ".center(w-4, "-"), curses.color_pair(1) | curses.A_BOLD)
            y += 1
            if y >= h - 6: break

        is_selected = (i == idx)
        style = curses.color_pair(6) | curses.A_REVERSE if is_selected else curses.A_NORMAL
        prefix = " > " if is_selected else "   "
        
        stdscr.attron(style)
        stdscr.addstr(y, 2, prefix + f"{opt['name']:23} ")
        stdscr.attroff(style)

        if opt["type"] == "value":
            try: val = opt["get"]()
            except: val = "ERR"
            val_str = f" [{str(val):<19}] "
            stdscr.addstr(val_str, curses.color_pair(3))
        elif opt["type"] == "toggle":
            try: state = opt["get"]()
            except: state = False
            color = curses.color_pair(2) if state else curses.color_pair(4)
            val_str = f" [{'ACTIVE' if state else 'OFF':<19}] "
            stdscr.addstr(val_str, color)
        elif opt["type"] == "action":
            stdscr.addstr(" [ EXECUTE           ] ", curses.color_pair(6))

        s_color = curses.color_pair(2) if opt.get("safety") == "SAFE" else curses.color_pair(3) if opt.get("safety") == "WARN" else curses.color_pair(4)
        stdscr.addstr(f" {opt.get('safety', ''):>7}", s_color)
        y += 1

    try:
        stdscr.addstr(h-5, 2, "─" * (w-4))
        stdscr.addstr(h-4, 2, "EXPLANATION:", curses.A_BOLD | curses.color_pair(6))
        stdscr.addstr(h-3, 4, OPTIONS[idx].get("desc", "")[:w-8], curses.A_ITALIC)
        stdscr.addstr(h-1, 2, "[↑↓] Navigate | [←→] Adjust | [ENTER] Change/Execute | [TAB] Change View | [R] Emergency Restore | [Q] Quit", curses.A_DIM)
    except:
        pass

def main(stdscr):
    try:
        curses.curs_set(0)
    except:
        pass
    stdscr.nodelay(1)
    
    delay_ms = 1000
    stdscr.timeout(delay_ms)
    
    try:
        curses.start_color()
        bg = curses.COLOR_BLACK
        try:
            curses.use_default_colors()
            bg = -1
        except:
            pass
        curses.init_pair(1, curses.COLOR_CYAN, bg)
        curses.init_pair(2, curses.COLOR_GREEN, bg)
        curses.init_pair(3, curses.COLOR_YELLOW, bg)
        curses.init_pair(4, curses.COLOR_RED, bg)
        curses.init_pair(5, curses.COLOR_BLACK, curses.COLOR_CYAN)
        curses.init_pair(6, curses.COLOR_BLUE, bg)
    except:
        pass
    
    dash = PowerDashboard()
    
    import sys
    active_view = 0
    if len(sys.argv) > 1 and "--monitor" in sys.argv:
        active_view = 1
    menu_idx = 0
    
    while True:
        h, w = stdscr.getmaxyx()
        if h < 18 or w < 78:
            stdscr.clear()
            stdscr.addstr(h // 2, max(0, (w - 38) // 2), "Terminal too small.", curses.color_pair(4) | curses.A_BOLD)
            key = stdscr.getch()
            if key == ord('q') or key == ord('Q'): break
            time.sleep(0.2)
            continue
            
        stdscr.clear()
        stats = dash.get_power_stats()
        dash.history.append(stats["total_w"])
        
        stdscr.attron(curses.color_pair(5))
        view_title = "GENERAL CONTROL PANEL" if active_view == 0 else "ENERGY MONITOR"
        stdscr.addstr(0, 0, f" ⚡ POWER CENTER EXTREME | {view_title} | [Tab] Change View | [R] Emergency Restore ".center(w, " "))
        stdscr.attroff(curses.color_pair(5))
        
        if active_view == 0:
            draw_view_menu(stdscr, menu_idx, h, w)
            dash.history = dash.history[-200:]
        else:
            processes = get_top_power_processes(stats["cpu_w"], stats["screen_w"], stats["total_w"])
            col1_w = min(55, max(40, int(w * 0.45)))
            
            stdscr.addstr(2, 2, "POWER MODE:", curses.A_BOLD)
            stdscr.addstr(2, 18, f" {get_current_mode()} ", curses.A_REVERSE)
            
            stdscr.addstr(4, 2, f"CONSUMPTION: {stats['total_w']:6.2f} W", curses.A_BOLD)
            
            proc_x = col1_w + 4
            stdscr.addstr(2, proc_x, "PROCESSES:", curses.A_BOLD)
            for idx, proc in enumerate(processes[:9]):
                stdscr.addstr(4 + idx, proc_x, f"{proc['pid']:>6} {proc['cpu']:>6}% {proc['power']:12.2f} W  {proc['name'][:15]}")
            
            if h > 16:
                dash.draw_graph(stdscr, 14, 2, h - 16, w - 4, dash.history)
        
        key = stdscr.getch()
        if key == ord('q') or key == ord('Q'): break
        elif key == ord('r') or key == ord('R'):
            apply_mode_restore()
            dash = PowerDashboard() # Reset dash history possibly, or just let it continue
        elif key == 9: active_view = 1 - active_view
        
        if active_view == 0:
            if key == curses.KEY_UP: menu_idx = (menu_idx - 1) % len(OPTIONS)
            elif key == curses.KEY_DOWN: menu_idx = (menu_idx + 1) % len(OPTIONS)
            elif key == curses.KEY_RIGHT:
                o = OPTIONS[menu_idx]
                if o["type"] == "value": 
                    try: stop_daemon(); o["set"](o["get"](), 1)
                    except: pass
            elif key == curses.KEY_LEFT:
                o = OPTIONS[menu_idx]
                if o["type"] == "value": 
                    try: stop_daemon(); o["set"](o["get"](), -1)
                    except: pass
            elif key in [10, 13, ord(' ')]:
                o = OPTIONS[menu_idx]
                try:
                    if o["type"] == "toggle": 
                        stop_daemon()
                        o["set"](not o["get"]())
                    elif o["type"] == "action": 
                        o["exec"]()
                    elif o["type"] == "value":
                        stop_daemon()
                        o["set"](o["get"](), 1)
                except:
                    pass


if __name__ == "__main__":
    import sys
    if os.geteuid() != 0:
        print("ERROR: Power Center Extreme requires root privileges to modify hardware states!")
        print("Please run with: sudo power-center")
        sys.exit(1)
        
    if len(sys.argv) > 1:
        if sys.argv[1] == "daemon":
            if os.geteuid() != 0:
                print("Must run daemon as root!")
                sys.exit(1)
            daemon_loop()
            sys.exit(0)
        elif sys.argv[1] == "mode":
            mode = sys.argv[2] if len(sys.argv) > 2 else ""
            if mode == "performance": apply_mode_performance()
            elif mode == "extreme": apply_mode_extreme()
            elif mode == "auto-extreme": apply_mode_autoextreme()
            elif mode == "restore": apply_mode_restore()
            else: print("Unknown mode. Use: performance, extreme, auto-extreme, restore")
            sys.exit(0)
            
    try:
        curses.wrapper(main)
    except KeyboardInterrupt:
        pass
