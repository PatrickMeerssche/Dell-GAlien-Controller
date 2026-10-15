import os
import shutil
import subprocess
import time

from PySide6.QtGui import QColor, QPainter, QPen, QPixmap
from PySide6.QtWidgets import QCheckBox, QGroupBox, QHBoxLayout, QLabel, QSizePolicy, QVBoxLayout, QWidget

try:
    import pynvml
    _HAS_PYNVML = True
except Exception:
    _HAS_PYNVML = False


class SensorsPanelMixin:
    # Sensors panel and polling.

    def _read_int_file(self, path):
        try:
            with open(path, "r", encoding="utf-8") as handle:
                return int(handle.read().strip())
        except Exception:
            return None

    def _find_cpu_energy_path(self):
        candidates = []
        for rapl_base in ("/sys/class/powercap", "/sys/devices/virtual/powercap"):
            if not os.path.isdir(rapl_base):
                continue
            for root, _dirs, files in os.walk(rapl_base):
                if "energy_uj" not in files:
                    continue

                score = 0
                name_path = os.path.join(root, "name")
                try:
                    with open(name_path, "r", encoding="utf-8") as handle:
                        name = handle.read().strip().lower()
                except OSError:
                    name = ""

                # Prefer package-level domains when available.
                if "package" in name:
                    score += 10
                if "intel-rapl" in root or "amd-rapl" in root:
                    score += 2

                energy_path = os.path.join(root, "energy_uj")
                candidates.append((score, energy_path))

        if not candidates:
            return None, None

        candidates.sort(key=lambda item: item[0], reverse=True)
        energy_path = candidates[0][1]
        max_range = self._read_int_file(os.path.join(os.path.dirname(energy_path), "max_energy_range_uj"))
        return energy_path, max_range

    def _find_hwmon_power_path(self, name_keys):
        base = "/sys/class/hwmon"
        if not os.path.isdir(base):
            return None
        for hwmon in sorted(os.listdir(base)):
            name_path = os.path.join(base, hwmon, "name")
            try:
                with open(name_path, "r", encoding="utf-8") as handle:
                    name = handle.read().strip().lower()
            except OSError:
                continue
            if not any(key in name for key in name_keys):
                continue
            for candidate in ("power1_average", "power1_input"):
                power_path = os.path.join(base, hwmon, candidate)
                if os.path.isfile(power_path):
                    return power_path
        return None

    def _init_power_sources(self):
        if hasattr(self, "_power_sources_ready") and self._power_sources_ready:
            return

        self._power_sources_ready = True
        self._cpu_energy_path = None
        self._cpu_energy_max_uj = None
        self._cpu_power_path = None
        self._cpu_power_last_w = None
        self._gpu_power_path = None
        self._cpu_energy_last_uj = None
        self._cpu_energy_last_ts = None
        self._nvidia_smi_path = None
        self._gpu_power_last_w = None
        self._gpu_power_last_ts = 0.0
        self._nvml_ready = False
        self._nvml_handle = None

        # Prefer package-level RAPL energy (derived watts) for minimal overhead.
        self._cpu_energy_path, self._cpu_energy_max_uj = self._find_cpu_energy_path()

        if self._cpu_energy_path is None:
            self._cpu_power_path = self._find_hwmon_power_path(("coretemp", "k10temp", "cpu"))

        # Prefer NVIDIA hwmon for discrete GPU power, fall back to other GPU drivers.
        self._gpu_power_path = self._find_hwmon_power_path(("nvidia", "amdgpu", "i915", "xe", "intel_gpu"))
        if self._gpu_power_path is None and _HAS_PYNVML:
            try:
                pynvml.nvmlInit()
                if pynvml.nvmlDeviceGetCount() > 0:
                    self._nvml_handle = pynvml.nvmlDeviceGetHandleByIndex(0)
                    self._nvml_ready = True
            except Exception:
                self._nvml_ready = False
                self._nvml_handle = None
        if self._gpu_power_path is None and not self._nvml_ready:
            self._nvidia_smi_path = shutil.which("nvidia-smi")

    def _read_hwmon_power_watts(self, path):
        try:
            with open(path, "r", encoding="utf-8") as handle:
                value = int(handle.read().strip())
            return value / 1_000_000.0
        except Exception:
            return None

    def _read_cpu_power_watts(self):
        if self._cpu_energy_path:
            energy_uj = self._read_int_file(self._cpu_energy_path)
            if energy_uj is None:
                return None

            now = time.monotonic()
            if self._cpu_energy_last_uj is None:
                self._cpu_energy_last_uj = energy_uj
                self._cpu_energy_last_ts = now
                return self._cpu_power_last_w

            delta_uj = energy_uj - self._cpu_energy_last_uj
            delta_s = now - (self._cpu_energy_last_ts or now)

            # Counter can wrap/reset; compensate when max range is known.
            if delta_uj < 0 and self._cpu_energy_max_uj and self._cpu_energy_max_uj > 0:
                delta_uj += self._cpu_energy_max_uj

            self._cpu_energy_last_uj = energy_uj
            self._cpu_energy_last_ts = now

            if delta_uj <= 0 or delta_s <= 0:
                return self._cpu_power_last_w

            self._cpu_power_last_w = (delta_uj / delta_s) / 1_000_000.0
            return self._cpu_power_last_w

        if self._cpu_power_path:
            return self._read_hwmon_power_watts(self._cpu_power_path)
        return None

    def _read_gpu_power_watts(self):
        if self._gpu_power_path:
            return self._read_hwmon_power_watts(self._gpu_power_path)
        if self._nvml_ready and self._nvml_handle is not None:
            try:
                power_mw = pynvml.nvmlDeviceGetPowerUsage(self._nvml_handle)
                return power_mw / 1000.0
            except Exception:
                self._nvml_ready = False
        if self._nvidia_smi_path:
            now = time.monotonic()
            # Rate-limit to keep overhead low.
            if (now - self._gpu_power_last_ts) < 1.0:
                return self._gpu_power_last_w
            try:
                result = subprocess.run(
                    [
                        self._nvidia_smi_path,
                        "--query-gpu=power.draw",
                        "--format=csv,noheader,nounits",
                    ],
                    capture_output=True,
                    text=True,
                    timeout=0.5,
                )
                if result.returncode == 0:
                    line = result.stdout.strip().splitlines()[0].strip()
                    self._gpu_power_last_w = float(line)
                    self._gpu_power_last_ts = now
                    return self._gpu_power_last_w
            except Exception:
                return self._gpu_power_last_w
        return None

    def _format_watts(self, value):
        if value is None:
            return "N/A"
        return f"{value:.1f} W"

    def _smooth_series(self, values, window=4):
        if len(values) < 2:
            return values
        window = max(2, int(window))
        smoothed = []
        for i in range(len(values)):
            start = max(0, i - window + 1)
            chunk = values[start:i + 1]
            smoothed.append(int(round(sum(chunk) / len(chunk))))
        return smoothed

    def _draw_sparkline(self, values, width=160, height=28, line_color=QColor('#66ccff'), show_min_max=False):
        # Allow dynamic sizing when width/height are None or zero
        if not width:
            width = 160
        if not height:
            height = 28
        pix = QPixmap(int(width), int(height))
        pix.fill(QColor('#000000'))
        if not values:
            return pix
        painter = QPainter(pix)
        # Subtle gridlines
        grid_pen = QPen(QColor('#1f1f1f'))
        grid_pen.setWidth(1)
        painter.setPen(grid_pen)
        for frac in (0.25, 0.5, 0.75):
            y = int(frac * (height - 1))
            painter.drawLine(0, y, int(width) - 1, y)
        for frac in (0.25, 0.5, 0.75):
            x = int(frac * (width - 1))
            painter.drawLine(x, 0, x, int(height) - 1)

        pen = QPen(line_color)
        pen.setWidth(2)
        painter.setPen(pen)
        # Scale values to widget height
        mn = min(values)
        mx = max(values)
        rng = mx - mn if mx != mn else 1
        pts = []
        step = width / max(1, (len(values) - 1))
        for i, v in enumerate(values):
            x = int(i * step)
            y = int((1 - (v - mn) / rng) * (height - 4)) + 2
            pts.append((x, y))
        # Draw polyline
        for i in range(len(pts) - 1):
            painter.drawLine(pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1])

        if show_min_max:
            text_pen = QPen(QColor('#cfcfcf'))
            painter.setPen(text_pen)
            painter.drawText(4, 12, f"↑{int(mx)}°C")
            painter.drawText(4, int(height) - 4, f"↓{int(mn)}°C")
        painter.end()
        return pix

    def _update_sparklines(self):
        try:
            # Compute sizes based on current widget geometry so sparklines resize
            def size_for(widget):
                w = widget.width() or 120
                h = widget.height() or 28
                return max(80, w), max(20, h)

            accent = '#66ccff'
            if hasattr(self, '_theme_color'):
                try:
                    accent = self._theme_color()
                except Exception:
                    pass

            w, h = size_for(self.spark_cpu)
            self.spark_cpu.setPixmap(
                self._draw_sparkline(
                    self._smooth_series(list(self.cpu_history)),
                    w,
                    h,
                    line_color=QColor(accent),
                    show_min_max=True,
                )
            )
            w, h = size_for(self.spark_gpu)
            self.spark_gpu.setPixmap(
                self._draw_sparkline(
                    self._smooth_series(list(self.gpu_history)),
                    w,
                    h,
                    line_color=QColor(accent),
                    show_min_max=True,
                )
            )
            self.is_resizing = False
        except Exception:
            pass

    def _sensors_autorefresh_changed(self, enabled):
        self.sensors_auto_refresh = bool(enabled)
        self.settings.setValue("Sensors Auto Refresh", str(self.sensors_auto_refresh))
        if hasattr(self, 'timer') and self.timer is not None:
            if self.sensors_auto_refresh:
                # fixed 0.5s interval
                self.timer.setInterval(int(self.sensors_interval * 1000))
                self.timer.start()
            else:
                self.timer.stop()

    def showEvent(self, event):
        # Resume polling when window becomes visible
        try:
            if self.sensors_auto_refresh and hasattr(self, 'timer') and self.timer is not None:
                self.timer.setInterval(int(self.sensors_interval * 1000))
                self.timer.start()
        except Exception:
            pass
        return super().showEvent(event)

    def hideEvent(self, event):
        # Pause polling when window hidden to save resources
        try:
            if hasattr(self, 'timer') and self.timer is not None:
                self.timer.stop()
        except Exception:
            pass
        return super().hideEvent(event)

    def resizeEvent(self, event):
        # Keep sparklines in sync with the available width
        try:
            if hasattr(self, 'spark_cpu'):
                if hasattr(self, 'sparkline_resize_timer'):
                    self.is_resizing = True
                    self.sparkline_resize_timer.start()
                else:
                    self._update_sparklines()
        except Exception:
            pass
        return super().resizeEvent(event)

    def _create_sensors_group(self):
        # Small panel that shows CPU/GPU temps and fan RPMs.
        group = QGroupBox("Sensors")
        layout = QHBoxLayout()
        layout.setSpacing(12)

        # Current value labels
        self.sensor_cpu_label = QLabel("CPU Temp: N/A")
        self.sensor_gpu_label = QLabel("GPU Temp: N/A")
        self.sensor_fan1_label = QLabel("CPU Fan: N/A RPM")
        self.sensor_cpu_watts_label = QLabel("CPU Power: N/A")
        self.sensor_fan2_label = QLabel("GPU Fan: N/A RPM")
        self.sensor_gpu_watts_label = QLabel("GPU Power: N/A")

        # Sparklines (resize with window)
        self.spark_cpu = QLabel()
        self.spark_gpu = QLabel()
        for spark in (self.spark_cpu, self.spark_gpu):
            spark.setMinimumHeight(64)
            spark.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

        left_widget = QWidget()
        left_layout = QVBoxLayout(left_widget)
        left_layout.setContentsMargins(0, 0, 0, 0)
        left_layout.setSpacing(8)
        left_layout.addWidget(self.sensor_cpu_label)
        left_layout.addWidget(self.spark_cpu)
        left_layout.addWidget(self.sensor_gpu_label)
        left_layout.addWidget(self.spark_gpu)

        right_widget = QWidget()
        right_layout = QVBoxLayout(right_widget)
        right_layout.setContentsMargins(0, 0, 0, 0)
        right_layout.setSpacing(8)
        right_layout.addWidget(self.sensor_fan1_label)
        right_layout.addWidget(self.sensor_cpu_watts_label)
        right_layout.addWidget(self.sensor_fan2_label)
        right_layout.addWidget(self.sensor_gpu_watts_label)

        # Auto-refresh controls
        controls_widget = QWidget()
        controls_hbox = QHBoxLayout(controls_widget)
        controls_hbox.setContentsMargins(0, 0, 0, 0)
        controls_hbox.setSpacing(8)
        self.sensors_autorefresh_checkbox = QCheckBox("Auto refresh (0.5s)")
        self.sensors_autorefresh_checkbox.setChecked(self.sensors_auto_refresh)
        controls_hbox.addWidget(self.sensors_autorefresh_checkbox)
        # Interval is fixed at 0.5s — no user control required
        controls_hbox.addStretch(1)

        right_layout.addWidget(controls_widget)
        right_layout.addStretch(1)

        layout.addWidget(left_widget, 3)
        layout.addWidget(right_widget, 1)

        # Wire controls
        self.sensors_autorefresh_checkbox.toggled.connect(self._sensors_autorefresh_changed)
        # Apply current setting immediately
        try:
            self._sensors_autorefresh_changed(self.sensors_auto_refresh)
        except Exception:
            pass

        group.setLayout(layout)
        # Initialize empty sparklines
        self._update_sparklines()
        return group

    def get_rpm_and_temp(self):
        # Only poll when auto-refresh is enabled, window is visible, and sensors panel is visible.
        if not (self.sensors_auto_refresh and self.isVisible() and hasattr(self, 'sensors_group') and self.sensors_group.isVisible()):
            return
        self._init_power_sources()
        # Proceed with polling
        # Get current rpm and temp from ACPI and update both the Power/Fans
        # panel and the new Sensors panel.
        try:
            fan1_rpm = self.acpi_call("get_fan1_rpm")
            cpu_temp = self.acpi_call("get_cpu_temp")
            fan2_rpm = self.acpi_call("get_fan2_rpm")
            gpu_temp = self.acpi_call("get_gpu_temp")
        except Exception as err:
            # ACPI call failed; record and skip this cycle
            self.last_acpi_response = f"error: {err.__class__.__name__}"
            self._refresh_diagnostics()
            return

        # Parse and update existing Power and Fans RPM labels
        try:
            cpu_val = int(cpu_temp, 0)
            gpu_val = int(gpu_temp, 0)
            fan1_val = int(fan1_rpm, 0)
            fan2_val = int(fan2_rpm, 0)
        except Exception:
            # Non-numeric response; ignore this cycle
            return

        try:
            self.fan1_current.setText("{} RPM, {} °C".format(fan1_val, cpu_val))
            self.fan2_current.setText("{} RPM, {} °C".format(fan2_val, gpu_val))
        except Exception:
            pass

        current_values = (cpu_val, gpu_val, fan1_val, fan2_val)
        cpu_watts_text = self._format_watts(self._read_cpu_power_watts())
        gpu_watts_text = self._format_watts(self._read_gpu_power_watts())
        if current_values == self.last_sensor_values:
            # Always refresh power labels; skip history/redraw if thermal values are unchanged.
            self.sensor_cpu_watts_label.setText(f"CPU Watts: {cpu_watts_text}")
            self.sensor_gpu_watts_label.setText(f"GPU Watts: {gpu_watts_text}")
            return
        self.last_sensor_values = current_values

        # Append history and update sparklines/labels
        self.cpu_history.append(cpu_val)
        self.gpu_history.append(gpu_val)

        try:
            self.sensor_cpu_label.setText(f"CPU Temp: {cpu_val} °C")
            self.sensor_gpu_label.setText(f"GPU Temp: {gpu_val} °C")
            self.sensor_fan1_label.setText(f"CPU Fan: {fan1_val} RPM")
            self.sensor_fan2_label.setText(f"GPU Fan: {fan2_val} RPM")
            self.sensor_cpu_watts_label.setText(f"CPU Watts: {cpu_watts_text}")
            self.sensor_gpu_watts_label.setText(f"GPU Watts: {gpu_watts_text}")
            # no last-update display (fixed-rate refresh)
            if not self.is_resizing:
                self._update_sparklines()
        except Exception:
            pass
