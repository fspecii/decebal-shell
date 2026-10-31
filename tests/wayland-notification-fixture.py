#!/usr/bin/env python3
"""DRAFT (never executed): unit tests of the trace and signal parsers of tests/e2e/wayland_notifications.py.
No compositor, bus, process or desktop connection is used."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("notify_fixture", Path(__file__).parent / "e2e/wayland_notifications.py")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)

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
