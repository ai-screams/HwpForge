"""Automatic conversion and source protection contracts."""

from __future__ import annotations

import ast
import inspect
import json
import os
import warnings
from pathlib import Path
from typing import Any, cast

import _stub
import pytest
from _hwp5_input import source

from hwpforge import Document, HwpForgeError, HwpForgeIOError, _hwpforge

READS = [
    ("inspect", {"styles": True}),
    ("outline", {}),
    ("fields", {}),
    ("validate", {}),
    ("read", {"section": 0}),
    ("stamp_plan", {}),
    ("to_md", {}),
    ("to_json", {}),
    ("export_section", {"section": 0}),
]
EDITS = ["fill", "set_cell", "insert_para", "delete_para", "stamp"]


def edit_args(name, converted):
    candidate = _hwpforge.stamp_plan(converted)["text"][0]
    return {
        "fill": {"values": {"name": "새값"}},
        "set_cell": {"table": 0, "at": "0,0", "text": "새셀"},
        "insert_para": {"section": 0, "anchor": 0, "text": "삽입"},
        "delete_para": {"section": 0, "indexes": [1]},
        "stamp": {
            "request": [
                {
                    **{k: candidate[k] for k in ("section", "path", "span", "marker")},
                    "action": {"field": {"name": "stamped", "hint": None}},
                }
            ]
        },
    }[name]


@pytest.mark.parametrize(("name", "kwargs"), READS)
@pytest.mark.parametrize("loss", [False, True])
def test_reads_match_explicit_conversion(name, kwargs, loss):
    # 이것을 실패시키는 것: 각 읽기 함수의 변환 연결 또는 conversion 메타 제거.
    data = source(loss)
    converted, _ = _hwpforge.convert_hwp5(data)
    direct = getattr(_hwpforge, name)(data, **kwargs)
    explicit = getattr(_hwpforge, name)(converted, **kwargs)
    metadata = direct[1] if name == "to_md" else direct
    conversion = metadata.pop("conversion")
    assert conversion["source_format"] == "hwp5"
    assert bool(conversion["warnings"]) == loss
    assert direct == explicit
    doc_result = getattr(Document(data), name)(**kwargs)
    report = getattr(doc_result, "report", doc_result)
    assert report["conversion"] == conversion


@pytest.mark.parametrize("roles", ["base", "revised", "both"])
def test_diff_converts_each_input(roles):
    # 이것을 실패시키는 것: base/revised 변환 연결 또는 역할별 메타 제거.
    data = source(True)
    converted, _ = _hwpforge.convert_hwp5(data)
    base = converted if roles == "revised" else data
    revised = converted if roles == "base" else data
    direct = Document(base).diff(Document(revised))
    conversion = cast("dict[str, Any]", direct.pop("conversion"))
    assert set(conversion) == ({"base", "revised"} if roles == "both" else {roles})
    assert all(value["source_format"] == "hwp5" for value in conversion.values())
    assert direct == _hwpforge.diff(converted, revised=converted)


@pytest.mark.parametrize("name", EDITS)
@pytest.mark.parametrize("loss", [False, True])
def test_edit_changes_content_and_matches_explicit_conversion(name, loss):
    # 이것을 실패시키는 것: 각 편집 변환/허용 전달 제거 또는 손실 거부 우회.
    data = source(loss)
    converted, _ = _hwpforge.convert_hwp5(data)
    kwargs = edit_args(name, converted)
    operation = getattr(_hwpforge, name)
    if loss:
        with pytest.raises(HwpForgeError) as caught:
            operation(data, **kwargs)
        assert caught.value.code == "HWP5_CONVERSION_LOSS"
        assert (
            cast("dict[str, Any]", caught.value.details)["warnings"][0]["code"] == "DROPPED_CONTROL"
        )
    direct, report = operation(data, **kwargs, accept_conversion_loss=loss)
    conversion = report.pop("conversion")
    assert conversion["source_format"] == "hwp5"
    explicit, expected_report = operation(converted, **kwargs)
    assert direct == explicit
    assert direct != converted
    assert report == expected_report
    expected = {
        "fill": "새값",
        "set_cell": "새셀",
        "insert_para": "삽입",
        "delete_para": "삭제 대상",
        "stamp": "stamped",
    }[name]
    reread = json.dumps(_hwpforge.to_json(direct), ensure_ascii=False)
    assert (expected in reread) == (name != "delete_para")
    result = getattr(Document(data), name)(**kwargs, accept_conversion_loss=loss)
    assert result.document.to_bytes() == direct
    assert result.report["conversion"] == conversion


@pytest.mark.parametrize(
    ("data", "code"),
    [
        (b"bad", "NO_VALUES"),
        (source(True), "NO_VALUES"),
        (bytes.fromhex("d0cf11e0a1b11ae1"), "HWP5_DECODE_FAILED"),
    ],
)
def test_error_order(data, code):
    # 이것을 실패시키는 것: 변환을 ops 뒤로 이동 또는 손실 거부를 ops 앞으로 이동.
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.fill(data, values={})
    assert caught.value.code == code


@pytest.mark.parametrize("prefix", [b"", b"prefix", bytes.fromhex("d0cf11e0a1b11ae1")])
def test_hwpx_prefixes_keep_result_and_no_conversion(prefix):
    # 이것을 실패시키는 것: 무변환 결과에 conversion: None 추가.
    converted, _ = _hwpforge.convert_hwp5(source())
    direct = _hwpforge.fields(prefix + converted)
    assert "conversion" not in direct
    assert direct == _hwpforge.fields(converted)


@pytest.mark.parametrize("name", ["from_json", "patch", "restyle"])
def test_deferred_inputs_still_refuse(name):
    # 이것을 실패시키는 것: 보류 함수에 자동 변환 연결.
    data = source()
    converted, _ = _hwpforge.convert_hwp5(data)
    calls = {
        "from_json": lambda: _hwpforge.from_json(
            json.dumps(_hwpforge.to_json(converted)["document"]), base=data
        ),
        "patch": lambda: _hwpforge.patch(
            data,
            section=0,
            patch=json.dumps(_hwpforge.export_section(converted, section=0)["section"]),
        ),
        "restyle": lambda: _hwpforge.restyle(data, preset="default"),
    }
    with pytest.raises(HwpForgeError) as caught:
        calls[name]()
    assert caught.value.code == "HWP5_INPUT"


@pytest.mark.parametrize("suffix", [".hwp", ".HWP", ".HwP"])
def test_save_refuses_hwp_extension(tmp_path, suffix):
    # 이것을 실패시키는 것: 확장자 가드 삭제 또는 lower 제거.
    target = tmp_path / ("output" + suffix)
    with pytest.raises(HwpForgeError, match="HWPX") as caught:
        Document(b"unchanged").save(target)
    assert caught.value.code == "INVALID_EXTENSION"
    assert not target.exists()


@pytest.mark.parametrize("alias", ["same", "absolute", "symlink", "hardlink", "case"])
@pytest.mark.parametrize("edited", [False, True])
def test_save_protects_source_aliases(tmp_path, monkeypatch, alias, edited):
    # 이것을 실패시키는 것: 원본 정체성 기억/상속, HWP5 판별, 정체성 가드 중 하나 제거.
    monkeypatch.chdir(tmp_path)
    path = Path("source.bin")
    original = source()
    path.write_bytes(original)
    doc = Document.open(path)
    if edited:
        doc = doc.insert_para(section=0, anchor=0, text="새 문단").document
    targets = {
        "same": path,
        "absolute": path.absolute(),
        "symlink": Path("link.hwpx"),
        "hardlink": Path("hard.hwpx"),
        "case": Path("SOURCE.BIN"),
    }
    target = targets[alias]
    if alias == "case" and not target.exists():
        pytest.skip("filesystem distinguishes case aliases")
    if alias == "symlink":
        target.symlink_to(path)
    if alias == "hardlink":
        os.link(path, target)
    with pytest.raises(HwpForgeError) as caught:
        doc.save(target)
    assert caught.value.code == "INVALID_OUTPUT_PATH"
    assert path.read_bytes() == original


def test_save_new_target_hwpx_in_place_and_bytes_only(tmp_path):
    # 이것을 실패시키는 것: 미존재 출력 거부, HWPX 제자리 저장 거부 또는 save 변환 생략.
    original = source()
    path = tmp_path / "source.hwp"
    path.write_bytes(original)
    output = tmp_path / "converted.hwpx"
    Document.open(path).save(output)
    expected, _ = _hwpforge.convert_hwp5(original)
    assert output.read_bytes() == expected
    Document.open(output).save(output)
    Document.from_bytes(original).save(output)
    assert output.read_bytes() == expected
    assert path.read_bytes() == original


def test_save_after_source_removed(tmp_path):
    # 이것을 실패시키는 것: 사라진 원본 경로를 조회하여 쓰기 실패로 처리.
    path = tmp_path / "source.hwp"
    original = source()
    path.write_bytes(original)
    # 원본을 지우기 전에 만들어 삭제된 inode를 받지 않게 한다.
    output = tmp_path / "existing.hwpx"
    output.write_bytes(b"old output")
    doc = Document.open(path)
    path.unlink()
    doc.save(output)
    assert output.read_bytes() == _hwpforge.convert_hwp5(original)[0]


@pytest.mark.parametrize("change", ["touch", "append"])
def test_save_refuses_alias_after_source_changed(tmp_path, change):
    # 이것을 실패시키는 것: 정체성 비교에 크기 또는 수정 시각 추가.
    path = tmp_path / "source.bin"
    path.write_bytes(source())
    alias = tmp_path / "keep.hwpx"
    os.link(path, alias)
    doc = Document.open(path)
    if change == "touch":
        status = path.stat()
        os.utime(path, ns=(status.st_atime_ns, status.st_mtime_ns + 1_000_000_000))
    else:
        with path.open("ab") as handle:
            handle.write(b"added")
    changed = alias.read_bytes()
    with pytest.raises(HwpForgeError) as caught:
        doc.save(alias)
    assert caught.value.code == "INVALID_OUTPUT_PATH"
    assert alias.read_bytes() == changed


@pytest.mark.parametrize("edit", [None, *EDITS])
@pytest.mark.parametrize(
    ("change", "destination", "refused"),
    [
        ("deleted", "alias", True),
        ("replaced", "alias", True),
        ("replaced", "source", False),
        ("replaced", "unrelated", False),
        ("deleted", "unrelated", False),
    ],
)
def test_save_protects_opened_file_after_path_change(tmp_path, edit, change, destination, refused):
    # 이것을 실패시키는 것: 저장 때 원본 경로 재조회, 정체성 미기억 또는 편집 상속 제거.
    path = tmp_path / "source.bin"
    original = source()
    path.write_bytes(original)
    alias = tmp_path / "keep.hwpx"
    os.link(path, alias)
    unrelated = tmp_path / "other.hwpx"
    unrelated.write_bytes(b"old output")
    doc = Document.open(path)
    converted, _ = _hwpforge.convert_hwp5(original)
    if edit is not None:
        doc = getattr(doc, edit)(**edit_args(edit, converted)).document
        # 후속 HWPX 편집에도 최초 파일의 정체성이 이어져야 한다.
        doc = doc.insert_para(section=0, anchor=0, text="후속 문단").document
    expected = converted if edit is None else doc.to_bytes()
    if change == "deleted":
        path.unlink()
    else:
        replacement = tmp_path / "replacement.bin"
        replacement.write_bytes(b"replacement")
        os.replace(replacement, path)
    target = {"alias": alias, "source": path, "unrelated": unrelated}[destination]
    if refused:
        with pytest.raises(HwpForgeError) as caught:
            doc.save(target)
        assert caught.value.code == "INVALID_OUTPUT_PATH"
    else:
        doc.save(target)
        assert target.read_bytes() == expected
    assert alias.read_bytes() == original
    if change == "replaced" and destination != "source":
        assert path.read_bytes() == b"replacement"


def test_open_remembers_the_read_handle(tmp_path, monkeypatch):
    # 이것을 실패시키는 것: 읽은 핸들의 fstat 대신 원본 경로 stat 사용.
    path = tmp_path / "source.bin"
    original = source()
    path.write_bytes(original)
    alias = tmp_path / "keep.hwpx"
    os.link(path, alias)
    replacement = tmp_path / "replacement.bin"
    replacement.write_bytes(b"replacement")
    builtin_open = open

    def replace_after_open(candidate, mode):
        handle = builtin_open(candidate, mode)
        os.replace(replacement, path)
        return handle

    with monkeypatch.context() as patcher:
        patcher.setattr("hwpforge.document.open", replace_after_open, raising=False)
        doc = Document.open(path)
    assert doc.to_bytes() == original
    with pytest.raises(HwpForgeError) as caught:
        doc.save(alias)
    assert caught.value.code == "INVALID_OUTPUT_PATH"
    assert alias.read_bytes() == original
    doc.save(path)
    assert path.read_bytes() == _hwpforge.convert_hwp5(original)[0]


def test_open_identity_failure(tmp_path, monkeypatch):
    # 이것을 실패시키는 것: fstat 실패 무시 또는 읽기 오류 매핑 밖으로 이동.
    path = tmp_path / "source.bin"
    path.write_bytes(source())

    def inaccessible_handle(fd):
        raise PermissionError("opened file identity unavailable")

    with monkeypatch.context() as patcher:
        patcher.setattr(os, "fstat", inaccessible_handle)
        with pytest.raises(HwpForgeIOError) as caught:
            Document.open(path)
    assert caught.value.code == "FILE_READ_FAILED"


def test_save_uses_only_output_identity(tmp_path, monkeypatch):
    # 이것을 실패시키는 것: 저장 시 원본 경로 stat 재도입과 원본 권한 오류 의존.
    path = tmp_path / "source.hwp"
    original = source()
    path.write_bytes(original)
    doc = Document.open(path)
    output = tmp_path / "existing.hwpx"
    output.write_bytes(b"old output")
    stat = os.stat
    calls = []

    def output_identity(candidate, *args, **kwargs):
        calls.append(candidate)
        if candidate != output:
            raise PermissionError("source identity unavailable")
        return stat(candidate, *args, **kwargs)

    with monkeypatch.context() as patcher:
        patcher.setattr(os, "stat", output_identity)
        doc.save(output)
    assert calls == [output]
    assert output.read_bytes() == _hwpforge.convert_hwp5(original)[0]


@pytest.mark.parametrize(("device_delta", "inode_delta"), [(0, 0), (1, 0), (0, 1)])
def test_save_identity_requires_device_and_inode(tmp_path, monkeypatch, device_delta, inode_delta):
    # 이것을 실패시키는 것: 정체성 비교에서 st_dev 또는 st_ino 생략.
    path = tmp_path / "source.bin"
    original = source()
    path.write_bytes(original)
    doc = Document.open(path)
    target = tmp_path / "out.hwpx"
    target.write_bytes(b"old output")
    identity = list(os.stat(path))
    identity[2] += device_delta
    identity[1] += inode_delta
    with monkeypatch.context() as patcher:
        patcher.setattr(os, "stat", lambda candidate: os.stat_result(identity))
        if device_delta == inode_delta == 0:
            with pytest.raises(HwpForgeError) as caught:
                doc.save(target)
            assert caught.value.code == "INVALID_OUTPUT_PATH"
        else:
            doc.save(target)
    expected = (
        b"old output" if device_delta == inode_delta == 0 else _hwpforge.convert_hwp5(original)[0]
    )
    assert target.read_bytes() == expected
    assert path.read_bytes() == original


def test_save_refuses_bytes_hwp_path(tmp_path, monkeypatch):
    # 이것을 실패시키는 것: bytes 경로를 fsdecode 대신 fspath로 확장자 비교.
    monkeypatch.chdir(tmp_path)
    with pytest.raises(HwpForgeError) as caught:
        Document(source()).save(cast("Any", b"x.hwp"))
    assert caught.value.code == "INVALID_EXTENSION"
    assert not (tmp_path / "x.hwp").exists()


def test_save_hwpx_reuses_input_bytes(tmp_path, generated_bytes):
    # 이것을 실패시키는 것: 무변환 입력의 새 PyBytes 반환 또는 Python 원본 재사용 제거.
    assert _hwpforge._prepare_save(generated_bytes) == (None, None)
    output = tmp_path / "unchanged.hwpx"
    Document(generated_bytes).save(output)
    assert output.read_bytes() == generated_bytes


def test_save_loss_policy_and_identity_failure(tmp_path, monkeypatch):
    # 이것을 실패시키는 것: save 손실 허용 무시/거부 우회 또는 정체성 조회 오류 삼키기.
    path = tmp_path / "source.bin"
    path.write_bytes(source(True))
    doc = Document.open(path)
    output = tmp_path / "out.hwpx"
    with pytest.raises(HwpForgeError) as caught:
        doc.save(output)
    assert caught.value.code == "HWP5_CONVERSION_LOSS"
    assert not output.exists()
    with pytest.warns(UserWarning, match="DROPPED_CONTROL"):
        doc.save(output, accept_conversion_loss=True)
    assert output.read_bytes() == _hwpforge.convert_hwp5(source(True))[0]

    def inaccessible(*args, **kwargs):
        raise PermissionError("identity unavailable")

    with monkeypatch.context() as patcher:
        patcher.setattr(os, "stat", inaccessible)
        with pytest.raises(HwpForgeIOError) as caught:
            doc.save(output, accept_conversion_loss=True)
    assert caught.value.code == "FILE_WRITE_FAILED"


@pytest.mark.parametrize("name", EDITS)
def test_edit_keyword_and_conversion_stub(name):
    # 이것을 실패시키는 것: FFI/스텁의 accept_conversion_loss 또는 conversion 선언 제거.
    parameter = inspect.signature(getattr(_hwpforge, name)).parameters["accept_conversion_loss"]
    assert parameter.default is False
    assert parameter.kind == inspect.Parameter.KEYWORD_ONLY
    node = next(n for n in _stub._module().body if getattr(n, "name", None) == name)
    assert "accept_conversion_loss" in [
        a.arg for a in cast("ast.FunctionDef", node).args.kwonlyargs
    ]
    report = {
        "fill": "FillReport",
        "set_cell": "SetCellReport",
        "insert_para": "StructuralReport",
        "delete_para": "StructuralReport",
        "stamp": "StampReport",
    }[name]
    assert "conversion" in _stub.typed_dict_keys(report)[1]


def test_source_survives_later_edits_and_changed_directory(tmp_path, monkeypatch):
    # 이것을 실패시키는 것: 원본 정체성 또는 후속 편집의 HWP5 출처 상속 제거.
    path = tmp_path / "source.bin"
    path.write_bytes(source())
    monkeypatch.chdir(tmp_path)
    doc = Document.open("source.bin").insert_para(section=0, anchor=0, text="first").document
    doc = doc.delete_para(section=0, indexes=[1]).document
    child = tmp_path / "child"
    child.mkdir()
    monkeypatch.chdir(child)
    with pytest.raises(HwpForgeError) as caught:
        doc.save(path)
    assert caught.value.code == "INVALID_OUTPUT_PATH"
    assert path.read_bytes() == source()


def test_save_broken_link_and_write_only_target(tmp_path):
    # 이것을 실패시키는 것: 없는 링크 대상을 거부하거나 출력 파일의 읽기 권한 요구.
    path = tmp_path / "source.bin"
    path.write_bytes(source())
    target = tmp_path / "target.hwpx"
    alias = tmp_path / "alias.hwpx"
    alias.symlink_to(target)
    doc = Document.open(path)
    doc.save(alias)
    expected = target.read_bytes()
    target.chmod(0o200)
    try:
        doc.save(target)
    finally:
        target.chmod(0o600)
    assert target.read_bytes() == expected


def test_conversion_report_shapes_and_existing_warnings():
    # 이것을 실패시키는 것: 읽기 conversion 스텁 또는 경고 stage/location 선언 제거.
    for name in [
        "Inspect",
        "Outline",
        "Fields",
        "Validate",
        "Read",
        "StampPlan",
        "ToMd",
        "ToJson",
        "ExportSection",
        "Diff",
    ]:
        assert "conversion" in _stub.typed_dict_keys(name + "Report")[1]
    assert _stub.typed_dict_keys("ConversionMeta")[0] == {"source_format", "warnings"}
    assert _stub.typed_dict_keys("ConversionWarning") == (
        {"stage", "code", "message"},
        {"location"},
    )
    assert _stub.typed_dict_keys("DiffConversionMeta")[1] == {"base", "revised"}


@pytest.mark.parametrize(
    ("base_kind", "expected"),
    [
        ("hwpx", [None, None, "HWP5_DECODE_FAILED"]),
        ("hwp5", [None, None, "HWP5_DECODE_FAILED"]),
        ("broken", ["DECODE_FAILED", "DECODE_FAILED", "DECODE_FAILED"]),
        ("cfb", ["HWP5_DECODE_FAILED", "HWP5_DECODE_FAILED", "HWP5_DECODE_FAILED"]),
    ],
)
@pytest.mark.parametrize(("revised_kind", "column"), [("hwpx", 0), ("hwp5", 1), ("cfb", 2)])
def test_diff_base_error_precedes_revised_conversion(base_kind, expected, revised_kind, column):
    # 이것을 실패시키는 것: revised 변환 실패 시 base 오류 확인 제거 또는 무조건 base 오류 반환.
    hwp5 = source()
    hwpx, _ = _hwpforge.convert_hwp5(hwp5)
    inputs = {
        "hwpx": hwpx,
        "hwp5": hwp5,
        "broken": b"broken base",
        "cfb": bytes.fromhex("d0cf11e0a1b11ae1"),
    }
    base, revised = Document(inputs[base_kind]), Document(inputs[revised_kind])
    if expected[column] is None:
        assert base.diff(revised)["identical"]
    else:
        with pytest.raises(HwpForgeError) as caught:
            base.diff(revised)
        assert caught.value.code == expected[column]


@pytest.mark.parametrize(
    ("loss", "memo_shape", "unknown_tag"),
    [(True, False, False), (False, True, False), (True, True, False), (False, False, True)],
)
def test_save_emits_each_conversion_warning_after_write(tmp_path, loss, memo_shape, unknown_tag):
    # 이것을 실패시키는 것: 저장 메타/경고 제거, 비손실 경고 필터, 코드/위치 누락.
    # 이것을 실패시키는 것: 메시지/경고 종류/stacklevel 또는 None 반환 변경.
    data = source(loss, memo_shape=memo_shape, unknown_tag=unknown_tag)
    doc = Document(data)
    expected = doc.fields()["conversion"]["warnings"]
    assert len(expected) == int(loss) + int(memo_shape) + int(unknown_tag)
    output = tmp_path / "saved.hwpx"
    with warnings.catch_warnings(record=True) as emitted:
        warnings.simplefilter("always")
        line = inspect.stack()[0].lineno + 1
        result = doc.save(output, accept_conversion_loss=loss or unknown_tag)
    assert result is None
    assert output.read_bytes() == _hwpforge.convert_hwp5(data)[0]
    assert len(emitted) == len(expected)
    for actual, warning in zip(emitted, expected):
        assert actual.category is UserWarning
        assert actual.filename == __file__
        assert actual.lineno == line
        location = warning.get("location")
        where = f"{location}: " if location else ""
        assert str(actual.message) == f"[{warning['code']}] {where}{warning['message']}"


def test_save_error_filter_raises_after_the_file_is_written(tmp_path):
    # 이것을 실패시키는 것: 쓰기 전에 경고하거나 경고를 HwpForgeError로 감싸기.
    output = tmp_path / "saved.hwpx"
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        with pytest.raises(UserWarning):
            Document(source(True)).save(output, accept_conversion_loss=True)
    assert output.exists()


@pytest.mark.parametrize("failure", ["loss", "write", "close"])
def test_save_failure_emits_no_conversion_warnings(tmp_path, monkeypatch, failure):
    # 이것을 실패시키는 것: 파일 쓰기/닫기 성공 전에 경고 방출 또는 손실 거부 우회.
    doc = Document(source(True))
    output = tmp_path / "saved.hwpx"
    if failure == "write":
        output = tmp_path / "missing" / "saved.hwpx"
    elif failure == "close":
        import io

        class FailedClose(io.BytesIO):
            def __exit__(self, exc_type, exc_val, exc_tb):
                super().__exit__(exc_type, exc_val, exc_tb)
                raise OSError("close failed")

        monkeypatch.setattr("builtins.open", lambda *args, **kwargs: FailedClose())
    with warnings.catch_warnings(record=True) as emitted:
        warnings.simplefilter("always")
        with pytest.raises(HwpForgeError) as caught:
            doc.save(output, accept_conversion_loss=failure != "loss")
    assert caught.value.code == (
        "HWP5_CONVERSION_LOSS" if failure == "loss" else "FILE_WRITE_FAILED"
    )
    assert emitted == []
    assert not output.exists()


@pytest.mark.parametrize("kind", ["hwp5", "hwpx"])
def test_save_clean_inputs_emit_no_warnings(tmp_path, kind):
    # 이것을 실패시키는 것: 경고 없는 변환 또는 HWPX 저장에 일반 경고 추가.
    data = source()
    hwpx, _ = _hwpforge.convert_hwp5(data)
    output = tmp_path / "saved.hwpx"
    with warnings.catch_warnings(record=True) as emitted:
        warnings.simplefilter("always")
        assert Document(data if kind == "hwp5" else hwpx).save(output) is None
    assert emitted == []
    assert output.read_bytes() == hwpx


@pytest.mark.parametrize("accept", [False, True])
def test_set_cell_conversion_acceptance_keeps_address_errors(accept):
    # 이것을 실패시키는 것: 없는 라벨을 첫 칸으로 보내 주소 오류를 우회하기.
    with pytest.raises(HwpForgeError) as caught:
        _hwpforge.set_cell(
            source(True), table=0, right_of="없는 라벨", text="값", accept_conversion_loss=accept
        )
    assert caught.value.code == "CELL_NOT_FOUND"
