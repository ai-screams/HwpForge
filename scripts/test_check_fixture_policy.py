"""Policy checks use only synthetic ZIP metadata and standard-library unittest."""

import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import check_fixture_policy as policy


class FixturePolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def package(self, name="문서 with spaces.hwpx", version='appVersion="12.0.0.0"'):
        with zipfile.ZipFile(self.root / name, "w") as package:
            package.writestr("version.xml", f"<version {version}/>")
            package.writestr("Contents/section0.xml", "deliberately not XML")
        return name

    # 이것을 실패시키는 것: startswith 를 동등 비교로 바꾸거나 본문 XML 검사 추가.
    def test_encoder_prefix_and_text_are_allowed(self):
        name = self.package(version='appVersion="12.0.0.0 build"')
        self.assertEqual(policy.check(self.root, [name, "notes.md"], {}), [])

    # 이것을 실패시키는 것: 금지 확장자 분기를 제거.
    def test_binary_formats_are_rejected(self):
        for name in ("document.hwp", "document.pdf", "document.PDF"):
            with self.subTest(name=name):
                self.package(name=name)
                self.assertEqual(len(policy.check(self.root, [name], {})), 1)

    # 이것을 실패시키는 것: appVersion 검사를 무조건 참으로 바꾸기.
    def test_foreign_or_missing_version_is_rejected(self):
        for version in ('appVersion="12.30.0.0"', "", 'appVersion="112.0.0.0"'):
            with self.subTest(version=version):
                self.assertEqual(
                    len(policy.check(self.root, [self.package(version=version)], {})), 1
                )

    # 이것을 실패시키는 것: 읽기 실패에서 위반을 기록하지 않기.
    def test_unreadable_metadata_is_rejected(self):
        name = "broken.hwpx"
        for value in (None, b"not a zip", b"<broken"):
            with self.subTest(value=value):
                if value == b"<broken":
                    with zipfile.ZipFile(self.root / name, "w") as package:
                        package.writestr("version.xml", value)
                elif value is not None:
                    (self.root / name).write_bytes(value)
                self.assertEqual(len(policy.check(self.root, [name], {})), 1)
        with zipfile.ZipFile(self.root / name, "w") as package:
            package.writestr("other.xml", "<version/>")
        self.assertEqual(len(policy.check(self.root, [name], {})), 1)

    # 이것을 실패시키는 것: 암호화 또는 미지원 압축의 읽기 오류를 위반에서 빼기.
    def test_unreadable_zip_methods_are_rejected(self):
        for error in (RuntimeError("encrypted"), NotImplementedError("compression")):
            with self.subTest(error=error):
                with patch.object(policy.zipfile, "ZipFile", side_effect=error):
                    self.assertEqual(len(policy.check(self.root, ["input.hwpx"], {})), 1)

    # 이것을 실패시키는 것: 예외 건너뛰기를 제거하거나 stale 검사를 제거.
    def test_exception_must_be_tracked_and_present(self):
        name = "contract.hwp"
        exceptions = {name: "error-contract input"}
        (self.root / name).write_bytes(b"contract")
        self.assertEqual(policy.check(self.root, [name], exceptions), [])
        self.assertIn("stale exception", policy.check(self.root, [], exceptions)[0])
        (self.root / name).unlink()
        self.assertIn("stale exception", policy.check(self.root, [name], exceptions)[0])

    # 이것을 실패시키는 것: NUL 구분을 줄 구분으로 바꾸거나 종료 코드를 뒤집기.
    def test_main_preserves_unicode_and_exit_status(self):
        paths = "한글\n문서.hwpx\0notes.md\0".encode()
        with patch.object(policy.subprocess, "check_output", return_value=paths) as git:
            with patch.object(policy, "check", return_value=[]) as check:
                self.assertEqual(policy.main(), 0)
                self.assertEqual(
                    check.call_args.args[1], ["한글\n문서.hwpx", "notes.md"]
                )
                self.assertEqual(git.call_args.args[0], ["git", "ls-files", "-z"])
            with patch.object(policy, "check", return_value=["bad"]):
                self.assertEqual(policy.main(), 1)


if __name__ == "__main__":
    unittest.main()
