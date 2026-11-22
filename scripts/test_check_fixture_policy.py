"""Policy checks use only synthetic files and standard-library unittest."""

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

    # 이것을 실패시키는 것: DOCUMENT_SUFFIXES 에서 .hwp 나 .pdf 를 빼기(서명이 없는 내용이라 확장자만 판정한다),
    # 또는 확장자와 서명이 함께 걸린 경로를 두 번 보고하기.
    def test_binary_formats_are_rejected(self):
        for name in ("document.hwp", "document.pdf", "document.PDF"):
            with self.subTest(name=name):
                (self.root / name).write_bytes(b"no signature here")
                self.assertEqual(len(policy.check(self.root, [name], {})), 1)
        zipped = self.package(name="zipped.hwp")
        self.assertEqual(len(policy.check(self.root, [zipped], {})), 1)

    # 이것을 실패시키는 것: SIGNATURES 에서 서명 하나를 빼기, 서명 검사를 확장자 검사로 되돌리기,
    # 또는 8바이트보다 짧은 파일을 건너뛰기.
    def test_document_signatures_are_rejected_under_any_name(self):
        with zipfile.ZipFile(self.root / "report.docx", "w") as package:
            package.writestr("word/document.xml", "<document/>")
        cases = {
            "report.docx": None,
            "legacy.dat": b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1rest",
            "package.dat": b"PK\x03\x04rest",
            "empty-archive.dat": b"PK\x05\x06rest",
            "spanned-archive.dat": b"PK\x07\x08rest",
            "report.txt": b"%PDF-1.7\nrest",
            "letter.txt": b"{\\rtf1\\ansi rest}",
            "short-pdf.dat": b"%PDF-",
            "short-archive.dat": b"PK\x05\x06",
            "short-rtf.dat": b"{\\rtf",
        }
        for name, data in cases.items():
            with self.subTest(name=name):
                if data is not None:
                    (self.root / name).write_bytes(data)
                violations = policy.check(self.root, [name], {})
                self.assertEqual(len(violations), 1)
                self.assertIn("EXCEPTIONS", violations[0])

    # 이것을 실패시키는 것: .hml 을 DOCUMENT_SUFFIXES 에서 빼거나 확장자를 소문자로 바꾸지 않기.
    def test_hancom_markup_text_is_rejected(self):
        (self.root / "form.HML").write_text(
            '<?xml version="1.0"?><HWPML/>', encoding="utf-8"
        )
        self.assertEqual(len(policy.check(self.root, ["form.HML"], {})), 1)

    # 이것을 실패시키는 것: 서명을 파일 맨 앞이 아닌 곳에서도 찾거나 모든 파일을 문서로 보기.
    def test_files_without_a_leading_signature_are_allowed(self):
        files = {
            "notes.md": b" %PDF- starts a PDF file and PK\x03\x04 a ZIP package\n",
            "image.png": b"\x89PNG\r\n\x1a\nrest",
            "font.ttf": b"\x00\x01\x00\x00rest",
            "truncated-ole.dat": b"\xd0\xcf\x11\xe0",
            "empty.txt": b"",
        }
        for name, data in files.items():
            (self.root / name).write_bytes(data)
        self.assertEqual(policy.check(self.root, sorted(files), {}), [])

    # 이것을 실패시키는 것: 심볼릭 링크를 따라가 판정하거나, 폴더·없는 경로의 읽기 오류로 멈추기.
    def test_links_folders_and_missing_paths_are_not_read_as_documents(self):
        (self.root / "real.dat").write_bytes(b"%PDF-1.7")
        (self.root / "link.md").symlink_to("real.dat")
        (self.root / "submodule").mkdir()
        self.assertEqual(
            policy.check(self.root, ["link.md", "missing.dat", "submodule"], {}), []
        )

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
        archive = "archive.zip"
        (self.root / archive).write_bytes(b"PK\x03\x04rest")
        self.assertEqual(
            policy.check(self.root, [archive], {archive: "error-contract input"}), []
        )
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
