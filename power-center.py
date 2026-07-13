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
    subprocess.run(f"echo {val} > {path}", shell=True)

# --- HARDWARE LOGIC ---

def _set_turbo(enabled):
    val = "0" if enabled else "1"
    if os.path.exists("/sys/devices/system/cpu/intel_pstate/no_turbo"):
        subprocess.run(f"echo {val} > /sys/devices/system/cpu/intel_pstate/no_turbo", shell=True)
    elif os.path.exists("/sys/devices/system/cpu/cpufreq/boost"):
        val_boost = "1" if enabled else "0"
        subprocess.run(f"echo {val_boost} > /sys/devices/system/cpu/cpufreq/boost", shell=True)

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
    subprocess.run(f"for i in /sys/devices/system/cpu/cpu*/cpufreq/scaling_max_freq; do [ -f \"$i\" ] && echo {mhz*1000} > \"$i\"; done", shell=True)


def _set_audio_powersave(enabled):
    val = "1" if enabled else "0"
    subprocess.run(f"for i in /sys/module/snd_*/parameters/power_save; do [ -f \"$i\" ] && echo {val} > \"$i\"; done", shell=True)

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
def _get_rapl_path():
    import glob
    paths = glob.glob("/sys/class/powercap/*rapl*0")
    return paths[0] if paths else "/sys/class/powercap/intel-rapl/intel-rapl:0"

def get_rapl_pl1():
    val = get_sys_val(f"{_get_rapl_path()}/constraint_0_power_limit_uw")
    return int(val) // 1000000 if val else 0

def set_rapl_pl1(watts):
    watts = max(5, min(115, watts))
    set_sys_val(f"{_get_rapl_path()}/constraint_0_power_limit_uw", watts * 1000000)

def get_rapl_pl2():
    val = get_sys_val(f"{_get_rapl_path()}/constraint_1_power_limit_uw")
    return int(val) // 1000000 if val else 0

def set_rapl_pl2(watts):
    watts = max(5, min(115, watts))
    set_sys_val(f"{_get_rapl_path()}/constraint_1_power_limit_uw", watts * 1000000)

EPP_PREFERENCES = ["default", "performance", "balance_performance", "balance_power", "power"]

def get_epp():
    val = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
    if val in EPP_PREFERENCES:
        return val
    return "default"

def set_epp(pref):
    if pref in EPP_PREFERENCES:
        subprocess.run(f"for i in /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference; do [ -f \"$i\" ] && echo {pref} > \"$i\"; done", shell=True)

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
def get_hypr_signature():
    paths = glob.glob("/run/user/1000/hypr/*_*")
    if paths:
        return os.path.basename(paths[0])
    paths = glob.glob("/tmp/hypr/*_*")
    if paths:
        return os.path.basename(paths[0])
    return ""

def run_hyprctl(cmd_args):
    sig = get_hypr_signature()
    if sig:
        cmd = f"sudo -u {REAL_USER} XDG_RUNTIME_DIR=/run/user/{USER_ID} HYPRLAND_INSTANCE_SIGNATURE={sig} hyprctl {cmd_args}"
    else:
        cmd = f"sudo -u {REAL_USER} XDG_RUNTIME_DIR=/run/user/{USER_ID} hyprctl {cmd_args}"
    return run(cmd)

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

def _set_hypr_monitor(fps):
    mon = _get_main_monitor()
    run_hyprctl(f'eval \'hl.monitor({{ output = "{mon}", mode = "1920x1080@{fps}", position = "auto", scale = 1 }})\'')

def set_hypr_effects(enabled):
    val = "true" if enabled else "false"
    lua = f"hl.config({{ animations = {{ enabled = {val} }}, decoration = {{ blur = {{ enabled = {val} }}, shadow = {{ enabled = {val} }} }} }})"
    run_hyprctl(f"eval '{lua}'")

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
    subprocess.run(f"iw dev {iface} set power_save {val}", shell=True)

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
        return "⚡ AUTO EXTREME (Dinámico)"
    
    epp = get_sys_val("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
    no_turbo = "0" if _get_turbo() else "1"
    
    num_cpus = get_num_cpus()
    active_cores = get_cores()
            
    if epp == "performance" and no_turbo == "0" and active_cores == num_cpus:
        return "⚡ PERFORMANCE (Máximo)"
    elif epp == "power" and active_cores == 1:
        return "🔋 EXTREME (Mínimo Fijo)"
    elif epp == "balance_performance" or epp == "default":
        return "♻  RESTAURAR (Balanceado)"
    else:
        return "⚙  PERSONALIZADO / MANUAL"

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
            time_str = "Cargando..."
            charge_full_val = get_sys_val("/sys/class/power_supply/BAT0/charge_full")
            if charge_full_val and current > 0.01:
                full = float(charge_full_val) / 10**6
                missing = full - charge_now
                if missing > 0:
                    hours = missing / current
                    h = int(hours)
                    m = int((hours - h) * 60)
                    time_str = f"Cargando ({h}h {m}m)"
        elif status.lower() == "full":
            time_str = "Lleno"

        cpu_w = 0.0
        rapl_path = f"{_get_rapl_path()}/energy_uj"
        uj_val = get_sys_val(rapl_path)
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
    stop_daemon()
    set_hypr_effects(True)
    _set_hypr_monitor(60)
    set_cores(get_num_cpus())
    set_rapl_pl1(45)
    set_rapl_pl2(65)
    set_epp("performance")
    _set_turbo(True)
    subprocess.run("brightnessctl set 100%", shell=True)
    set_kbd_backlight(True)
    set_wifi_powersave(False)
    subprocess.run("rfkill unblock bluetooth", shell=True)

def apply_mode_extreme():
    stop_daemon()
    set_hypr_effects(False)
    _set_hypr_monitor(48)
    set_cores(1)
    set_rapl_pl1(5)
    set_rapl_pl2(8)
    set_epp("power")
    _set_turbo(False)
    set_freq_limit(800)
    subprocess.run("brightnessctl set 2%", shell=True)
    set_kbd_backlight(False)
    set_wifi_powersave(True)
    subprocess.run("rfkill block bluetooth", shell=True)
    _set_audio_powersave(True)
    subprocess.run("echo 0 > /proc/sys/kernel/nmi_watchdog", shell=True)
    set_aspm_policy("powersupersave")
    subprocess.run("for i in /sys/bus/pci/devices/*/power/control; do echo auto > $i 2>/dev/null; done", shell=True)
    subprocess.run("for i in /sys/bus/usb/devices/*/power/control; do echo auto > $i 2>/dev/null; done", shell=True)

def apply_mode_restore():
    stop_daemon()
    set_hypr_effects(True)
    _set_hypr_monitor(60)
    set_cores(get_num_cpus())
    set_rapl_pl1(15)
    set_rapl_pl2(28)
    set_epp("balance_performance")
    _set_turbo(True)
    set_freq_limit(2400)
    subprocess.run("brightnessctl set 50%", shell=True)
    set_kbd_backlight(True)
    set_wifi_powersave(False)
    set_aspm_policy("default")

def apply_mode_autoextreme():
    stop_daemon()
    subprocess.run(f"python3 {os.path.abspath(__file__)} daemon &", shell=True)

def daemon_loop():
    with open(PID_FILE, 'w') as f:
        f.write(str(os.getpid()))
        
    set_hypr_effects(False)
    _set_hypr_monitor(48)
    subprocess.run("brightnessctl set 2%", shell=True)
    set_kbd_backlight(False)
    set_rapl_pl1(6)
    set_rapl_pl2(9)
    set_epp("power")
    _set_turbo(False)
    set_wifi_powersave(True)
    subprocess.run("rfkill block bluetooth", shell=True)
    set_aspm_policy("powersupersave")
    subprocess.run("echo 1500 > /proc/sys/vm/dirty_writeback_centisecs", shell=True)
    subprocess.run("echo 1500 > /proc/sys/vm/dirty_expire_centisecs", shell=True)
    
    total_cpus = get_num_cpus()
    
    import time
    while True:
        try:
            load = float(run("cat /proc/loadavg | awk '{print $1}'") or 0.5)
            
            if load < 0.8:
                target_cores, target_freq = 1, 800
            elif load < 1.5:
                target_cores, target_freq = 2, 1000
            elif load < 3.0:
                target_cores, target_freq = 3, 1200
            else:
                target_cores, target_freq = 4, 1400
                
            set_cores(target_cores)
            set_freq_limit(target_freq)
            
            active_win = ""
            try:
                out = run_hyprctl("activewindow -j")
                data = json.loads(out)
                active_win = data.get("class", "").lower()
            except:
                pass
            
            max_bright = float(run("brightnessctl m") or 100)
            if active_win in ["kitty", "foot", "alacritty", ""]:
                target_pct = 15 if load > 1.5 else 10
            else:
                if load > 2.0: target_pct = 30
                elif load > 0.8: target_pct = 20
                else: target_pct = 12
                
            target_val = int((max_bright * target_pct) / 100)
            target_val = max(1, target_val)
            
            curr_bright = int(run("brightnessctl g") or 0)
            if curr_bright != target_val:
                subprocess.run(f"brightnessctl set {target_val}", shell=True)
                
            time.sleep(5)
        except Exception as e:
            time.sleep(5)

OPTIONS = [
    {
        "name": "Cores Activos",
        "type": "value",
        "get": lambda: get_cores(),
        "set": lambda v, d: set_cores(v + d),
        "desc": "Control físico de núcleos. Menos núcleos = Menos consumo base y calor.",
        "safety": "SAFE"
    },
    {
        "name": "Freq CPU (MHz)",
        "type": "value",
        "get": lambda: get_freq_limit(),
        "set": lambda v, d: set_freq_limit(v + (d * 100)),
        "desc": "Límite máximo de frecuencia. 800-1200MHz es ideal para ahorro extremo.",
        "safety": "SAFE"
    },
    {
        "name": "EPP CPU Profile",
        "type": "value",
        "get": lambda: get_epp(),
        "set": lambda v, d: set_epp(EPP_PREFERENCES[(EPP_PREFERENCES.index(v) + d) % len(EPP_PREFERENCES)]),
        "desc": "Preferencias de energía y rendimiento del hardware de la CPU. 'power' es ultra ahorrativo.",
        "safety": "SAFE"
    },
    {
        "name": "RAPL PL1 (W)",
        "type": "value",
        "get": lambda: get_rapl_pl1(),
        "set": lambda v, d: set_rapl_pl1(v + d),
        "desc": "Límite de energía a largo plazo (PL1) para la CPU. 10W ideal en batería.",
        "safety": "SAFE"
    },
    {
        "name": "RAPL PL2 (W)",
        "type": "value",
        "get": lambda: get_rapl_pl2(),
        "set": lambda v, d: set_rapl_pl2(v + d),
        "desc": "Límite de energía a corto plazo (PL2) para la CPU. 15W ideal en batería.",
        "safety": "SAFE"
    },
    {
        "name": "PCIe ASPM Policy",
        "type": "value",
        "get": lambda: get_aspm_policy(),
        "set": lambda v, d: set_aspm_policy(ASPM_POLICIES[(ASPM_POLICIES.index(v) + d) % len(ASPM_POLICIES)]),
        "desc": "Active State Power Management de PCIe. 'powersupersave' ahorra más energía.",
        "safety": "SAFE"
    },
    {
        "name": "Turbo Boost",
        "type": "toggle",
        "get": lambda: _get_turbo(),
        "set": lambda s: _set_turbo(s),
        "desc": "Desactivar Turbo evita picos de calor y consumo masivo instantáneo.",
        "safety": "WARN"
    },
    {
        "name": "Freq iGPU (MHz)",
        "type": "value",
        "get": lambda: get_gpu_limit(),
        "set": lambda v, d: set_gpu_limit(v + (d * 50)),
        "desc": "Límite de la gráfica integrada. Reducir ahorra energía en Hyprland.",
        "safety": "SAFE"
    },
    {
        "name": "Brillo LCD (%)",
        "type": "value",
        "get": lambda: int((int(run("brightnessctl g"))/int(run("brightnessctl m")))*100) if run("brightnessctl m") else 0,
        "set": lambda v, d: subprocess.run(f"brightnessctl s {5 if d > 0 else -5}%", shell=True),
        "desc": "El panel es el mayor consumidor tras la CPU. Mantener bajo el 10%.",
        "safety": "SAFE"
    },
    {
        "name": "Luz Teclado",
        "type": "toggle",
        "get": lambda: get_kbd_backlight(),
        "set": lambda s: set_kbd_backlight(s),
        "desc": "Apagar la retroiluminación del teclado ahorra una pequeña fracción de vatio.",
        "safety": "SAFE"
    },
    {
        "name": "Efectos Hyprland",
        "type": "toggle",
        "get": lambda: get_hypr_animations(),
        "set": lambda s: set_hypr_effects(s),
        "desc": "Desactivar animaciones y blur libera a la GPU de trabajo innecesario.",
        "safety": "SAFE"
    },
    {
        "name": "Bluetooth",
        "type": "toggle",
        "get": lambda: "Soft blocked: no" in run("rfkill list bluetooth"),
        "set": lambda s: subprocess.run(f"rfkill {'unblock' if s else 'block'} bluetooth", shell=True),
        "desc": "Corte de radio Bluetooth. Evita que el chip busque dispositivos.",
        "safety": "SAFE"
    },
    {
        "name": "WiFi Radio",
        "type": "toggle",
        "get": lambda: "enabled" in run("nmcli radio wifi"),
        "set": lambda s: subprocess.run(f"nmcli radio wifi {'on' if s else 'off'}", shell=True),
        "desc": "Apagar la radio WiFi por completo. Ahorro masivo si no necesita internet.",
        "safety": "WARN"
    },
    {
        "name": "WiFi Power Save",
        "type": "toggle",
        "get": lambda: get_wifi_powersave(),
        "set": lambda s: set_wifi_powersave(s),
        "desc": "Activa el modo de ahorro de energía de la tarjeta WiFi. Reduce el consumo pero puede elevar la latencia.",
        "safety": "SAFE"
    },
    {
        "name": "Audio Power Save",
        "type": "toggle",
        "get": lambda: _get_audio_powersave(),
        "set": lambda s: _set_audio_powersave(s),
        "desc": "Suspende el chip de audio tras 1s de inactividad. Evita 'pop' de estática.",
        "safety": "SAFE"
    },
    {
        "name": "Autosuspend PCI/USB",
        "type": "toggle",
        "get": lambda: "auto" in run("cat /sys/bus/pci/devices/*/power/control 2>/dev/null | head -n 1"),
        "set": lambda s: [subprocess.run(f"for i in /sys/bus/pci/devices/*/power/control; do echo {'auto' if s else 'on'} > \"$i\" 2>/dev/null; done", shell=True), subprocess.run(f"for i in /sys/bus/usb/devices/*/power/control; do echo {'auto' if s else 'on'} > \"$i\" 2>/dev/null; done", shell=True)],
        "desc": "Suspende puertos USB y líneas PCIe inactivas. Puede afectar periféricos.",
        "safety": "DANGER"
    },
    {
        "name": "Watchdog Kernel",
        "type": "toggle",
        "get": lambda: get_sys_val("/proc/sys/kernel/nmi_watchdog") == "1",
        "set": lambda s: set_sys_val("/proc/sys/kernel/nmi_watchdog", "1" if s else "0"),
        "desc": "Desactivar interrupciones de seguridad del kernel. Reduce wakeups de CPU.",
        "safety": "WARN"
    },
    {
        "name": "VM Writeback (s)",
        "type": "value",
        "get": lambda: int(get_sys_val("/proc/sys/vm/dirty_writeback_centisecs") or 500) // 100,
        "set": lambda v, d: [set_sys_val("/proc/sys/vm/dirty_writeback_centisecs", max(1, v + d) * 100), set_sys_val("/proc/sys/vm/dirty_expire_centisecs", max(1, v + d) * 100)],
        "desc": "Intervalo de guardado de disco virtual. Valores altos reducen el uso del SSD.",
        "safety": "WARN"
    },
    {
        "name": "Purga de Procesos",
        "type": "action",
        "exec": lambda: subprocess.run("pkill -f 'brave|discord|telegram|code|electron'", shell=True),
        "desc": "Cierra aplicaciones pesadas (Brave, Code, Discord, etc) para liberar RAM y CPU.",
        "safety": "DANGER"
    },
    {
        "name": "⚡ MODO PERFORMANCE",
        "type": "action",
        "exec": apply_mode_performance,
        "desc": "TURBO BOOST + todos los cores + freq máxima + GPU máxima. Consumo elevado.",
        "safety": "WARN"
    },
    {
        "name": "🔋 MODO EXTREME",
        "type": "action",
        "exec": apply_mode_extreme,
        "desc": "Solo 1 core a 800MHz, GPU mínima, todo apagado. Máximo ahorro de batería.",
        "safety": "WARN"
    },
    {
        "name": "⚡ MODO AUTO EXTREME",
        "type": "action",
        "exec": apply_mode_autoextreme,
        "desc": "Extreme baseline + regulación dinámica de cores (1-4) y freq (800-1400MHz) según carga. Super tacaño.",
        "safety": "SAFE"
    },
    {
        "name": "♻  MODO RESTAURAR",
        "type": "action",
        "exec": apply_mode_restore,
        "desc": "Restaura valores predeterminados balanceados. 8 cores, freq normal, GPU normal.",
        "safety": "SAFE"
    }
]

# --- UNIFIED DASHBOARD INTERACTION ---
def draw_view_menu(stdscr, idx, h, w):
    # Stats header
    draw = get_power()
    bat = get_battery()
    temp = get_temp()
    stdscr.addstr(2, 2, "ESTADO DEL SISTEMA:", curses.A_BOLD)
    stdscr.addstr(3, 4, f"BATERÍA: {bat}%", curses.color_pair(2 if bat and int(bat) > 20 else 4))
    stdscr.addstr(3, 20, f"CONSUMO: {draw:.2f}W", curses.color_pair(3))
    stdscr.addstr(3, 40, f"TEMP: {temp}°C", curses.color_pair(4 if temp.isdigit() and int(temp) > 80 else 3))
    auto_status = "ACTIVE" if os.path.exists(PID_FILE) else "OFF"
    stdscr.addstr(4, 4, f"CORES: {get_cores()}/{get_num_cpus()} | CPU: {get_freq_limit()}MHz | EPP: {get_epp()} | PL1: {get_rapl_pl1()}W")
    stdscr.addstr(4, 75, f"AUTO-EXT: {auto_status}", curses.color_pair(2 if auto_status == "ACTIVE" else 4))

    # Controls Menu
    stdscr.addstr(6, 2, "CONTROLES DE HARDWARE (Usa Flechas / Enter):", curses.A_BOLD)
    for i, opt in enumerate(OPTIONS):
        if 7 + i >= h - 6:
            break
        style = curses.color_pair(5) if i == idx else curses.A_NORMAL
        stdscr.attron(style)
        stdscr.addstr(7 + i, 4, f" {opt['name']:20} ")
        stdscr.attroff(style)

        if opt["type"] == "value":
            val = opt["get"]()
            stdscr.addstr(f" [{str(val):<19}] ", curses.color_pair(3))
        elif opt["type"] == "toggle":
            state = opt["get"]()
            color = curses.color_pair(2) if state else curses.color_pair(4)
            stdscr.addstr(f" [{'ACTIVO' if state else 'OFF':<19}] ", color)
        elif opt["type"] == "action":
            stdscr.addstr(" [ EJECUTAR          ] ", curses.color_pair(4))

        # Safety Tag
        s_color = curses.color_pair(2) if opt["safety"] == "SAFE" else curses.color_pair(3) if opt["safety"] == "WARN" else curses.color_pair(4)
        stdscr.addstr(f" {opt['safety']:>7}", s_color)

    # Explanation and quick footer
    stdscr.addstr(h-5, 2, "─" * (w-4))
    stdscr.addstr(h-4, 2, "EXPLICACIÓN:", curses.A_BOLD)
    stdscr.addstr(h-3, 4, OPTIONS[idx]["desc"][:w-8], curses.A_ITALIC)
    try:
        stdscr.addstr(h-1, 2, "[↑↓] Navegar | [←→] Ajustar | [ENTER] Acción | [Tab] Alternar Vista Monitor | [Q] Salir", curses.A_DIM)
    except:
        pass

def main(stdscr):
    try:
        curses.curs_set(0)
    except:
        pass
    stdscr.nodelay(1)
    
    # Shared settings
    delay_ms = 1000
    stdscr.timeout(delay_ms)
    
    # Colors initialization
    try:
        curses.start_color()
        bg = curses.COLOR_BLACK
        try:
            curses.use_default_colors()
            bg = -1
        except:
            pass
        
        curses.init_pair(1, curses.COLOR_CYAN, bg)     # CPU / Titles / Header
        curses.init_pair(2, curses.COLOR_GREEN, bg)    # Low / Efficient
        curses.init_pair(3, curses.COLOR_YELLOW, bg)   # Medium / Warning
        curses.init_pair(4, curses.COLOR_RED, bg)      # High / Danger
        curses.init_pair(5, curses.COLOR_BLACK, curses.COLOR_CYAN)   # Title Highlight
        curses.init_pair(6, curses.COLOR_BLUE, bg)     # Others component
    except:
        pass
    
    dash = PowerDashboard()
    
    # Mode tracker: 0 = Control Panel (power_center), 1 = Live Power Monitor
    import sys
    active_view = 0
    if len(sys.argv) > 1 and "--monitor" in sys.argv:
        active_view = 1
    menu_idx = 0
    
    while True:
        h, w = stdscr.getmaxyx()
        
        # Guard small screens
        if h < 18 or w < 78:
            stdscr.clear()
            stdscr.addstr(h // 2, max(0, (w - 38) // 2), "Terminal muy pequeña. Agrándala para ver.", curses.color_pair(4) | curses.A_BOLD)
            key = stdscr.getch()
            if key == ord('q') or key == ord('Q'):
                break
            time.sleep(0.2)
            continue
            
        stdscr.clear()
        
        # Common readings
        stats = dash.get_power_stats()
        dash.history.append(stats["total_w"])
        
        # --- TITLE HEADER BAR ---
        stdscr.attron(curses.color_pair(5))
        view_title = "PANEL DE CONTROL GENERAL" if active_view == 0 else f"MONITOR DE ENERGÍA EN VIVO ({delay_ms}ms)"
        stdscr.addstr(0, 0, f" ⚡ POWER CENTER EXTREME | {view_title} | [Tab] Cambiar Vista ".center(w, " "))
        stdscr.attroff(curses.color_pair(5))
        
        # --- RENDER VIEW ---
        if active_view == 0:
            # Control Panel View
            draw_view_menu(stdscr, menu_idx, h, w)
            # Limit history queue growth
            dash.history = dash.history[-200:]
        else:
            # Live Power Monitor View
            processes = get_top_power_processes(stats["cpu_w"], stats["screen_w"], stats["total_w"])
            
            # Width and layout calculation
            col1_w = int(w * 0.45)
            col1_w = max(40, min(col1_w, 55))
            graph_w = w - 6
            available_cols = graph_w - 15
            
            # Keep history tailored for current window size
            dash.history = dash.history[-max(600, available_cols * 2):]
            
            # Column 1: Info Metrics
            mode = get_current_mode()
            stdscr.addstr(2, 2, "MODO DE ENERGÍA:", curses.A_BOLD)
            stdscr.addstr(2, 18, f" {mode} ", curses.color_pair(2 if "AUTO" in mode or "REST" in mode else 3) | curses.A_REVERSE)
            
            stdscr.addstr(4, 2, f"Batería: {stats['capacity']}% [", curses.A_BOLD)
            bar_w = 12
            filled = int(stats["capacity"] / 100 * bar_w)
            for i in range(bar_w):
                if i < filled:
                    color = curses.color_pair(2) if stats["capacity"] > 30 else curses.color_pair(3) if stats["capacity"] > 15 else curses.color_pair(4)
                    stdscr.addstr("█", color)
                else:
                    stdscr.addstr("░", curses.A_DIM)
            stdscr.addstr("]")
            stdscr.addstr(f" {stats['status'][:4]}", curses.color_pair(2 if stats['status'] == 'Full' or stats['status'] == 'Charging' else 3))
            stdscr.addstr(5, 2, f"⏱  {stats['time_str']} restante", curses.A_BOLD)
            
            total_color = curses.color_pair(2) if stats['total_w'] < 7.0 else curses.color_pair(3) if stats['total_w'] < 13.0 else curses.color_pair(4)
            stdscr.addstr(7, 2, "CONSUMO PC:  ", curses.A_BOLD)
            stdscr.addstr(7, 14, f"{stats['total_w']:6.2f} W", total_color | curses.A_BOLD)
            stdscr.addstr(7, 24, f"({stats['voltage']:.1f}V | {stats['current']:.2f}A)", curses.A_DIM)

            stdscr.addstr(9, 2, "🔌 CPU:      ", curses.A_BOLD)
            stdscr.addstr(9, 14, f"{stats['cpu_w']:5.2f} W", curses.color_pair(1))
            stdscr.addstr(10, 2, "🖥  Panel:    ", curses.A_BOLD)
            stdscr.addstr(10, 14, f"{stats['screen_w']:5.2f} W", curses.color_pair(3))
            stdscr.addstr(11, 2, "⚙  Otros HW:  ", curses.A_BOLD)
            stdscr.addstr(11, 14, f"{stats['others_w']:5.2f} W", curses.color_pair(6))
            
            # Column 2: Processes
            proc_x = col1_w + 4
            stdscr.addstr(2, proc_x, "PROCESOS CON MAYOR IMPACTO DE ENERGÍA:", curses.A_BOLD)
            stdscr.addstr(3, proc_x, "  PID    %CPU   ESTIMADO (W)   COMMAND", curses.A_UNDERLINE | curses.A_DIM)
            
            for idx, proc in enumerate(processes[:9]):
                proc_name = proc["name"][:20]
                row_y = 4 + idx
                if row_y >= 13 or row_y >= h - 2:
                    break
                p_style = curses.color_pair(4) if proc["power"] > 4.0 else curses.color_pair(3) if proc["power"] > 1.5 else curses.A_NORMAL
                stdscr.addstr(row_y, proc_x, f"{proc['pid']:>6} {proc['cpu']:>6}% {proc['power']:12.2f} W  {proc_name}", p_style)
                
            # Bottom graph
            divider_y = 14
            if h > divider_y + 4:
                stdscr.addstr(divider_y, 2, "─" * (w - 4), curses.A_DIM)
                graph_y = divider_y + 1
                graph_h = h - graph_y - 2
                
                if graph_w > 10 and graph_h > 2:
                    stdscr.addstr(graph_y + 1, 2, "GRÁFICO DE CONSUMO HISTÓRICO:", curses.A_BOLD)
                    dash.draw_graph(stdscr, graph_y + 2, 2, graph_h - 2, graph_w, dash.history)
            
            try:
                stdscr.addstr(h - 1, 2, f"Navegación: [Q] Salir | [+] Sumar tiempo (+100ms) | [-] Restar tiempo (-100ms) | [Tab] Volver al panel", curses.A_DIM)
            except:
                pass

        # Keyboard Interactivity
        key = stdscr.getch()
        
        if key == curses.KEY_RESIZE:
            continue
            
        if key == ord('q') or key == ord('Q'):
            break
        elif key == 9: # Tab key maps to decimal ASCII 9
            # Toggle between views
            active_view = 1 - active_view
        
        # View specific bindings
        if active_view == 0:
            if key == curses.KEY_UP:
                menu_idx = (menu_idx - 1) % len(OPTIONS)
            elif key == curses.KEY_DOWN:
                menu_idx = (menu_idx + 1) % len(OPTIONS)
            elif key == curses.KEY_RIGHT:
                o = OPTIONS[menu_idx]
                if o["type"] == "value": o["set"](o["get"](), 1)
            elif key == curses.KEY_LEFT:
                o = OPTIONS[menu_idx]
                if o["type"] == "value": o["set"](o["get"](), -1)
            elif key in [10, 13, ord(' ')]:
                o = OPTIONS[menu_idx]
                if o["type"] == "toggle": o["set"](not o["get"]())
                elif o["type"] == "action": o["exec"]()
        else:
            if key in [ord('+'), ord('='), 43, 61]:
                delay_ms = min(5000, delay_ms + 100)
                stdscr.timeout(delay_ms)
            elif key in [ord('-'), ord('_'), 45, 95]:
                delay_ms = max(100, delay_ms - 100)
                stdscr.timeout(delay_ms)
                
    curses.endwin()

if __name__ == "__main__":
    import sys
    if len(sys.argv) > 1:
        if sys.argv[1] == "daemon":
            if os.geteuid() != 0:
                print("Must run daemon as root!")
                sys.exit(1)
            daemon_loop()
            sys.exit(0)
        elif sys.argv[1] == "mode":
            if os.geteuid() != 0:
                print("Must run mode changes as root (sudo power-center mode <name>)")
                sys.exit(1)
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
