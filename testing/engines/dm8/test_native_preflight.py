import unittest

from native_preflight import control, version_banner


class NativePreflightTests(unittest.TestCase):
    def test_control_uses_private_data_directory_and_generated_secret(self):
        text = control("Dm" + "a1" * 20)
        self.assertIn("system_path=/opt/dmdbms/data", text)
        self.assertNotIn("/tmp", text)
        with self.assertRaises(ValueError):
            control("newline\ninjection")

    def test_process_success_or_client_banner_is_not_server_acceptance(self):
        text = "DM Database Server 64 V8\n03134284604-20260707-335949-20228"
        self.assertEqual(version_banner(text), "03134284604-20260707-335949-20228")
        for invalid in ("disql V8", text + "\n[-70025]:Connection not opened.",
                        text + "\nDISQL-10036: connection lost"):
            with self.assertRaises(ValueError):
                version_banner(invalid)
