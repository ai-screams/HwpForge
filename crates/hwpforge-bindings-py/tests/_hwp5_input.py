"""Synthetic CFB records matching the CLI input-editing fixture, without external files."""

import struct


def record(tag: int, level: int, payload: bytes) -> bytes:
    return struct.pack("<I", tag | level << 10 | len(payload) << 20) + payload


def paragraph(text: str) -> bytes:
    wire = text.encode("utf-16le")
    header = struct.pack("<I", len(wire) // 2) + bytes(18)
    return record(0x42, 0, header) + record(0x43, 1, wire) + record(0x44, 1, bytes(8))


def source(loss: bool = False) -> bytes:
    """Build the same text, click-here field, table and unknown-control records as CLI."""
    field_id = int.from_bytes(b"%clk", "big")

    def marker(code, ident):
        return struct.pack("<HI8xH", code, ident, code)

    text = marker(3, field_id) + "기존값".encode("utf-16le") + marker(4, 0)
    command = "Clickhere:set:0:Direction:wstring:0: HelpState:wstring:0:  ".encode("utf-16le")
    control = struct.pack("<I4xBH", field_id, 9, len(command) // 2) + command + bytes(8)
    name = bytes([0x1B, 2, 1, 0, 0, 0, 0, 0x40, 1, 0, 4, 0]) + "name".encode("utf-16le")
    field = (
        record(0x42, 0, bytes(22))
        + record(0x43, 1, text)
        + record(0x44, 1, bytes(8))
        + record(0x47, 1, control)
        + record(0x57, 2, name)
    )
    table_control = b" lbt" + bytes(36)
    table = bytes(4) + struct.pack("<HH", 1, 1) + bytes(12)
    cell = struct.pack("<I8xHHii8x", 1, 1, 1, 9000, 3000)
    table_wire = (
        paragraph("표")
        + record(0x47, 1, table_control)
        + record(0x4D, 2, table)
        + record(0x48, 2, cell)
        + record(0x42, 2, bytes(22))
        + record(0x43, 3, "셀".encode("utf-16le"))
        + record(0x44, 3, bytes(8))
    )
    section = (
        paragraph("이름: (   )") + paragraph("삭제 대상") + paragraph("마지막") + field + table_wire
    )
    section += record(0x47, 1, b"zzzz") * loss
    header = b"HWP Document File\0" + bytes(14) + struct.pack("<I", 0x05000205) + bytes(220)
    info = record(0x15, 0, bytes(68)) + record(0x19, 0, bytes(42)) + record(0x1A, 0, bytes(14))
    return compound(header, info, section)


def compound(header: bytes, info: bytes, section: bytes) -> bytes:
    """Pack three small streams into a CFB v3 mini stream."""
    end, free = 0xFFFFFFFE, 0xFFFFFFFF
    streams = [header, info, section]
    mini = b""
    chains = []
    starts = []
    for stream in streams:
        start = len(mini) // 64
        count = (len(stream) + 63) // 64
        starts.append(start)
        chains.extend([*range(start + 1, start + count), end])
        mini += stream.ljust(count * 64, b"\0")
    count = (len(mini) + 511) // 512
    fat = [0xFFFFFFFD, 2, end, end, *range(5, 4 + count), end]
    fat += [free] * (128 - len(fat))
    cfb_header = bytearray(512)
    cfb_header[:8] = bytes.fromhex("d0cf11e0a1b11ae1")
    struct.pack_into("<HHHHH", cfb_header, 24, 0x3E, 3, 0xFFFE, 9, 6)
    struct.pack_into("<IIIIIIIII", cfb_header, 40, 0, 1, 1, 0, 4096, 3, 1, end, 0)
    struct.pack_into("<109I", cfb_header, 76, 0, *([free] * 108))

    def entry(name, kind, left=free, right=free, child=free, start=end, size=0):
        wire = bytearray(128)
        encoded = (name + "\0").encode("utf-16le")
        wire[: len(encoded)] = encoded
        struct.pack_into("<HBBIII", wire, 64, len(encoded), kind, 1, left, right, child)
        struct.pack_into("<IQ", wire, 116, start, size)
        return wire

    directory = (
        entry("Root Entry", 5, child=2, start=4, size=len(mini))
        + entry("FileHeader", 2, start=starts[0], size=len(header))
        + entry("DocInfo", 2, right=3, start=starts[1], size=len(info))
        + entry("BodyText", 1, right=1, child=4)
        + entry("Section0", 2, start=starts[2], size=len(section))
    )
    return (
        bytes(cfb_header)
        + struct.pack("<128I", *fat)
        + directory.ljust(1024, b"\0")
        + struct.pack("<128I", *chains, *([free] * (128 - len(chains))))
        + mini.ljust(count * 512, b"\0")
    )
