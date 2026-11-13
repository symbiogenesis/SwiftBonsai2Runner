"""Exercise exact-input CI cache admission without compiling or running Rust."""

import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

import native_ci_cache as cache


SOURCE = "a" * 64
IMAGE = "win11-arm64-20261113-10.0.0-environment"
COMPILER = "rustc 1.101.0-nightly (fixture)\nhost: aarch64-pc-windows-msvc\n"


def pe_image(machine=0xAA64, magic=0x20B, characteristics=2):
    """Construct inert PE header bytes; this fixture cannot execute."""
    value = bytearray(128)
    value[:2] = b"MZ"
    struct.pack_into("<I", value, 60, 64)
    value[64:68] = b"PE\0\0"
    struct.pack_into("<H", value, 68, machine)
    struct.pack_into("<H", value, 86, characteristics)
    struct.pack_into("<H", value, 88, magic)
    return value


class NativeCiCacheTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="bonsai-ci-cache-")
        self.addCleanup(self.temporary.cleanup)
        self.package = Path(self.temporary.name)
        self.executable = self.package / "bonsai-ci.exe"
        self.executable.write_bytes(pe_image())
        cache.create(self.package, SOURCE, IMAGE, COMPILER)

    def replace_receipt(self, **fields):
        path = self.package / "receipt.json"
        receipt = json.loads(path.read_text(encoding="utf-8"))
        receipt.update(fields)
        path.write_text(json.dumps(receipt), encoding="utf-8")

    def test_exact_receipt_and_native_bytes_pass(self):
        self.assertEqual(cache.verify(self.package, SOURCE, IMAGE), self.executable)

    def test_every_build_identity_mismatch_fails(self):
        for field, replacement in (("sourceId", "b" * 64), ("hostId", "another-image"),
                                   ("toolchain", "nightly-other"),
                                   ("compiler", "host: x86_64-pc-windows-msvc"),
                                   ("schemaVersion", True), ("schemaVersion", 2)):
            with self.subTest(field=field, replacement=replacement):
                cache.create(self.package, SOURCE, IMAGE, COMPILER)
                self.replace_receipt(**{field: replacement})
                with self.assertRaises(ValueError):
                    cache.verify(self.package, SOURCE, IMAGE)

    def test_changed_executable_and_wrong_native_format_fail(self):
        fixtures = (pe_image() + b"tampered", pe_image(machine=0x8664),
                    pe_image(magic=0x10B), pe_image(characteristics=0x2002),
                    pe_image(characteristics=0), b"not a PE", b"MZ" + b"\0" * 62)
        for value in fixtures:
            with self.subTest(value=value):
                self.executable.write_bytes(value)
                with self.assertRaises(ValueError):
                    cache.verify(self.package, SOURCE, IMAGE)

    def test_malformed_duplicate_missing_extra_and_invalid_digest_fail(self):
        path = self.package / "receipt.json"
        for value in ("{}", "[]", "null", "not-json", '{"schemaVersion":1,"schemaVersion":1}'):
            with self.subTest(value=value):
                path.write_text(value, encoding="utf-8")
                with self.assertRaises(ValueError):
                    cache.verify(self.package, SOURCE, IMAGE)
        cache.create(self.package, SOURCE, IMAGE, COMPILER)
        self.replace_receipt(sha256="not-a-digest")
        with self.assertRaises(ValueError):
            cache.verify(self.package, SOURCE, IMAGE)
        cache.create(self.package, SOURCE, IMAGE, COMPILER)
        self.replace_receipt(unexpected="field")
        with self.assertRaises(ValueError):
            cache.verify(self.package, SOURCE, IMAGE)

    def test_missing_inputs_and_package_members_fail(self):
        for source, image in (("", IMAGE), ("a" * 63, IMAGE), (SOURCE, ""), (SOURCE, "image\nother")):
            with self.subTest(source=source, image=image), self.assertRaises(ValueError):
                cache.verify(self.package, source, image)
        self.executable.unlink()
        with self.assertRaises(OSError):
            cache.verify(self.package, SOURCE, IMAGE)

    def test_cli_returns_failure_without_selecting_bad_bytes(self):
        self.executable.write_bytes(b"invalid")
        result = subprocess.run([sys.executable, "-B", str(Path(cache.__file__)), "verify",
                                 "--package", str(self.package), "--source", SOURCE, "--host", IMAGE],
                                capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn("CI bootstrap verification failed:", result.stderr)


if __name__ == "__main__":
    unittest.main()
