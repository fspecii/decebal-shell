#!/usr/bin/env python3
"""DRAFT (never executed): unit tests of the trace and signal parsers of tests/e2e/wayland_notifications.py.
No compositor, bus, process or desktop connection is used."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location("notify_fixture", Path(__file__).parent / "e2e/wayland_notifications.py")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)


class CleanupTests(unittest.TestCase):
    def test_a_child_error_does_not_skip_other_children_or_pidfds(self):
        rig = object.__new__(FIXTURE.Rig)
        a, b = Mock(pid=100), Mock(pid=101)
        a.poll.return_value = b.poll.return_value = None
        rig.children = [("a", a, 70), ("b", b, 71)]
        rig.failures = []
        b.wait.side_effect = OSError("injected wait error")
        with patch.object(FIXTURE.signal, "pidfd_send_signal") as send, patch.object(FIXTURE.os, "close") as close:
            rig.close()
        self.assertEqual(send.call_count, 2)
        a.wait.assert_called_once()
        self.assertEqual({call.args[0] for call in close.call_args_list}, {70, 71})
        self.assertTrue(any("cleanup wait b" in f for f in rig.failures))

    def test_termination_bypasses_phase_error_handler(self):
        with self.assertRaises(KeyboardInterrupt):
            FIXTURE.interrupted(15, None)


GOOD = """
[1.0][rs] -> zwlr_layer_shell_v1@6.get_layer_surface(zwlr_layer_surface_v1@19, wl_surface@18, nil, 2, Some("decebal-notifications"))
[1.1][rs] -> zwlr_layer_surface_v1@19.set_anchor(9)
[1.2][rs] -> zwlr_layer_surface_v1@19.set_exclusive_zone(0)
[1.3][rs] -> zwlr_layer_surface_v1@19.set_keyboard_interactivity(0)
[1.4][rs] -> wl_compositor@4.create_region(wl_region@20)
[1.5][rs] -> wl_surface@18.set_input_region(wl_region@20)
[1.6][rs] -> wl_region@20.destroy()
[1.7][rs] -> zwlr_layer_surface_v1@19.set_size(229, 66)
[1.8][rs] -> zwlr_layer_surface_v1@19.set_margin(8, 12, 0, 0)
[1.9][rs] -> wl_surface@18.commit()
[2.0][rs] <- zwlr_layer_surface_v1@19.configure, (7, 229, 66)
[2.1][rs] -> wl_surface@18.attach(wl_buffer@31, 0, 0)
[2.2][rs] -> wl_compositor@4.create_region(wl_region@32)
[2.3][rs] -> wl_region@32.add(0, 0, 229, 66)
[2.4][rs] -> wl_surface@18.set_input_region(wl_region@32)
[2.5][rs] -> wl_surface@18.commit()
"""

SIGNALS = """signal time=1.0 sender=:1.3 -> destination=(null destination) serial=9 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=ActionInvoked
   uint32 4
   string "reply"
signal time=1.1 sender=:1.3 -> destination=(null destination) serial=10 path=/org/freedesktop/Notifications; interface=org.freedesktop.Notifications; member=NotificationClosed
   uint32 4
   uint32 2
"""


class BannerContract(unittest.TestCase):
    def test_a_conforming_banner_passes(self):
        failures = []
        evidence = FIXTURE.check_banner_contract(GOOD, failures)
        self.assertEqual(failures, [])
        self.assertEqual(evidence[0]["anchor"], [9])
        self.assertEqual(evidence[0]["margins"], [["8", "12", "0", "0"]])

    def test_each_violation_is_reported(self):
        for broken, expected in (
            (GOOD.replace("set_anchor(9)", "set_anchor(1)"), "anchor"),
            (GOOD.replace("set_exclusive_zone(0)", "set_exclusive_zone(-1)"), "exclusive zone"),
            (GOOD.replace("set_keyboard_interactivity(0)", "set_keyboard_interactivity(1)"), "keyboard interactivity"),
            (GOOD.replace("set_input_region(wl_region@20)", "set_input_region(nil)", 1), "explicit input region"),
            (GOOD.replace("[1.4][rs] -> wl_compositor@4.create_region(wl_region@20)", "[1.4][rs] -> wl_region@20.add(0, 0, 5, 5)"), "not empty"),
            (GOOD.replace(", 2, Some(", ", 3, Some("), "Layer::Top"),
        ):
            failures = []
            FIXTURE.check_banner_contract(broken, failures)
            self.assertTrue(any(expected in f for f in failures), (expected, failures))

    def test_a_buffer_before_the_empty_region_is_reported(self):
        early = GOOD.replace("[1.4][rs] -> wl_compositor@4.create_region(wl_region@20)\n[1.5][rs] -> wl_surface@18.set_input_region(wl_region@20)",
                             "[1.4][rs] -> wl_surface@18.attach(wl_buffer@30, 0, 0)\n[1.5][rs] -> wl_surface@18.set_input_region(wl_region@20)")
        failures = []
        FIXTURE.check_banner_contract(early, failures)
        self.assertTrue(any("before its explicit empty region" in f for f in failures), failures)


class KeyboardAndHistory(unittest.TestCase):
    def test_keyboard_enter_on_a_banner_is_a_failure(self):
        trace = GOOD + "[3.0][rs] <- wl_keyboard@12.enter, (8, wl_surface@18, [])\n"
        failures = []
        FIXTURE.check_no_banner_keyboard(trace, failures)
        self.assertTrue(failures)
        failures = []
        FIXTURE.check_no_banner_keyboard(GOOD + "[3.0][rs] <- wl_keyboard@12.enter, (8, wl_surface@99, [])\n", failures)
        self.assertEqual(failures, [])

    def test_history_needs_an_overlay_panel_with_exclusive_keyboard_and_a_catcher_per_output(self):
        trace = """
[1.0][rs] -> zwlr_layer_shell_v1@6.get_layer_surface(zwlr_layer_surface_v1@40, wl_surface@39, wl_output@13, 2, Some("decebal-notifications-outside"))
[1.1][rs] -> zwlr_layer_shell_v1@6.get_layer_surface(zwlr_layer_surface_v1@43, wl_surface@42, wl_output@14, 2, Some("decebal-notifications-outside"))
[1.2][rs] -> zwlr_layer_shell_v1@6.get_layer_surface(zwlr_layer_surface_v1@46, wl_surface@45, nil, 3, Some("decebal-notifications"))
[1.3][rs] -> zwlr_layer_surface_v1@46.set_keyboard_interactivity(1)
"""
        failures = []
        self.assertEqual(FIXTURE.check_history_contract(trace, 2, failures), {"panels": 1, "catchers": 2})
        self.assertEqual(failures, [])
        failures = []
        FIXTURE.check_history_contract(trace.replace("set_keyboard_interactivity(1)", "set_keyboard_interactivity(0)"), 2, failures)
        self.assertTrue(any("exclusive" in f for f in failures))
        failures = []
        FIXTURE.check_history_contract(trace.replace("wl_output@14", "wl_output@13"), 2, failures)
        self.assertTrue(any("catchers on 1 outputs" in f for f in failures))


class Scales(unittest.TestCase):
    def test_integer_uses_buffer_scale_and_fractional_a_viewport(self):
        failures = []
        FIXTURE.check_scale_protocol("[1.0][rs] -> wl_surface@18.set_buffer_scale(2)", 2, failures)
        self.assertEqual(failures, [])
        FIXTURE.check_scale_protocol("[1.0][rs] -> wl_surface@18.set_buffer_scale(1)", 2, failures)
        self.assertTrue(failures)
        failures = []
        FIXTURE.check_scale_protocol("[1.0][rs] -> wp_viewport@21.set_destination(229, 66)", 1.5, failures)
        self.assertEqual(failures, [])
        FIXTURE.check_scale_protocol("[1.0][rs] -> wl_surface@18.set_buffer_scale(2)", 1.5, failures)
        self.assertTrue(failures)


def buffers(spec):
    """Trace lines: create+attach for each (buffer id, surface) in `spec`; negative ids destroy that buffer."""
    lines = []
    for n, item in enumerate(spec):
        if item < 0:
            lines.append(f"[{n}.0][rs] -> wl_buffer@{-item}.destroy()")
        else:
            lines.append(f"[{n}.0][rs] -> wl_shm_pool@23.create_buffer(wl_buffer@{item}, 0, 344, 100, 1376, 0)")
            lines.append(f"[{n}.1][rs] -> wl_surface@18.attach(wl_buffer@{item}, 0, 0)")
    return "\n".join(lines) + "\n"


def callbacks(spec):
    """Trace lines: a positive id requests a frame callback on surface 18, a negative one delivers its done event."""
    lines = []
    for n, item in enumerate(spec):
        if item > 0:
            lines.append(f"[{n}.0][rs] -> wl_surface@18.frame(wl_callback@{item})")
        else:
            lines.append(f"[{n}.0][rs] <- wl_callback@{-item}.done, (1234)")
    return "\n".join(lines) + "\n"


PRESENT = {
    "totals": {"max_buffers": 3, "max_callbacks": 2},
    "limits": {"buffers": 3, "callbacks": 2, "surface_bytes": 33554432, "total_bytes": 100663296},
    "total_bytes": 204800,
    "pool_bytes": 200000,
    "surfaces": [{"id": 1, "buffers": 3, "bytes": 200000, "alloc_bytes": 204800}],
}


class PresentationBounds(unittest.TestCase):
    def test_kernel_mapping_sample_counts_only_notification_shm(self):
        maps = "1000-3000 rw-s 0 00:01 1 /memfd:smithay-client-toolkit (deleted)\n"
        maps += "3000-8000 rw-p 0 00:00 0 [heap]\n"
        maps += "8000-9000 rw-s 0 00:01 2 /memfd:another-client (deleted)\n"
        maps += "a000-b000 rw-s 0 00:01 3 /memfd:smithay-client-toolkit (deleted)\n"
        self.assertEqual(FIXTURE.shm_mapping_bytes(maps), 3 * 4096)

    def test_total_retained_pool_cap_is_checked(self):
        failures = []
        FIXTURE.check_presentation_bounds("", dict(PRESENT, total_buffers=65), failures, "t")
        self.assertTrue(any("pool count" in f for f in failures))

    def test_buffers_alive_at_once_are_counted_per_surface_including_held_ones(self):
        peaks, created = FIXTURE.buffer_peaks(buffers([31, 32, 33, -31, 34]))
        self.assertEqual(peaks, {18: 3})
        self.assertEqual(created, 4)
        peaks, _ = FIXTURE.buffer_peaks(buffers([31, 32, 33, 34]))
        self.assertEqual(peaks, {18: 4})

    def test_a_recycled_protocol_id_is_a_new_buffer(self):
        peaks, created = FIXTURE.buffer_peaks(buffers([31, 32, 33, -31, 31, 31]))
        self.assertEqual((peaks, created), ({18: 3}, 5))

    def test_callbacks_outstanding_are_counted_and_released_by_done(self):
        peaks, requested = FIXTURE.callback_peaks(callbacks([40, 41, -40, 42, -41]))
        self.assertEqual((peaks, requested), ({18: 2}, 3))
        peaks, _ = FIXTURE.callback_peaks(callbacks([40, 41, 42]))
        self.assertEqual(peaks, {18: 3})

    def test_bounds_fail_on_the_wire_and_in_the_daemons_own_counters(self):
        failures = []
        FIXTURE.check_presentation_bounds(buffers([31, 32, 33]) + callbacks([40, 41]), PRESENT, failures, "t")
        self.assertEqual(failures, [])
        failures = []
        FIXTURE.check_presentation_bounds(buffers([31, 32, 33, 34]), None, failures, "t")
        self.assertTrue(any("4 shm buffers" in f for f in failures), failures)
        failures = []
        FIXTURE.check_presentation_bounds(callbacks([40, 41, 42]), None, failures, "t")
        self.assertTrue(any("3 frame callbacks" in f for f in failures), failures)
        failures = []
        bad = dict(PRESENT, totals={"max_buffers": 4, "max_callbacks": 2}, total_bytes=10**9)
        FIXTURE.check_presentation_bounds("", bad, failures, "t")
        self.assertTrue(any("past its own limits" in f for f in failures) and any("budget" in f for f in failures), failures)
        failures = []
        pools = dict(PRESENT, pool_bytes=PRESENT["total_bytes"] + 1)
        FIXTURE.check_presentation_bounds("", pools, failures, "t")
        self.assertTrue(any("pools total" in f for f in failures), failures)
        failures = []
        big = dict(PRESENT, surfaces=[{"id": 1, "buffers": 3, "bytes": 200000, "alloc_bytes": 33554433}])
        FIXTURE.check_presentation_bounds("", big, failures, "t")
        self.assertTrue(any("shm bytes" in f for f in failures), failures)


class Signals(unittest.TestCase):
    def test_dbus_monitor_text_is_parsed(self):
        signals = FIXTURE.parse_signals(SIGNALS)
        self.assertEqual(signals, [("ActionInvoked", [4, "reply"]), ("NotificationClosed", [4, 2])])
        self.assertEqual(FIXTURE.closed_reasons(signals), {4: 2})
        self.assertEqual(FIXTURE.actions_invoked(signals), [(4, "reply")])


class Geometry(unittest.TestCase):
    BANNER = {"rect": [1500, 12, 344, 100], "buttons": [{"key": "reply", "rect": [60.0, 70.0, 100.0, 24.0]}], "close_rect": [300.0, 10.0, 18.0, 18.0]}

    def test_button_and_close_centres_are_global_logical_pixels(self):
        x, y = FIXTURE.button_center(self.BANNER, "reply", 1.5)
        self.assertAlmostEqual(x, (1500 + 60 + 50) / 1.5)
        self.assertAlmostEqual(y, (12 + 70 + 12) / 1.5)
        x, y = FIXTURE.close_center(self.BANNER, 2.0, origin=(1280, 0))
        self.assertAlmostEqual(x, 1280 + (1500 + 309) / 2.0)
        self.assertAlmostEqual(y, (12 + 19) / 2.0)


if __name__ == "__main__":
    unittest.main()
