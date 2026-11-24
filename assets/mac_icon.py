#!/usr/bin/env python3
"""把蓝底分支图做成 macOS 图标：图案缩到画布的 80%，底色透明。"""

import struct
import sys
import zlib


def read_png(path):
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("不是 PNG: " + path)
    pos = 8
    width = height = None
    raw = b""
    while pos < len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        kind = data[pos + 4 : pos + 8]
        chunk = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, bit, color, *_ = struct.unpack(">IIBBBBB", chunk)
            if bit != 8 or color != 6:
                raise SystemExit("只接受 8 位 RGBA PNG")
        elif kind == b"IDAT":
            raw += chunk
        elif kind == b"IEND":
            break
        pos += 12 + length
    rows = decode(zlib.decompress(raw), width, height)
    return width, height, rows


def decode(raw, width, height):
    stride = width * 4
    rows = []
    prev = bytearray(stride)
    index = 0

    def paeth(left, up, up_left):
        estimate = left + up - up_left
        da, db, dc = abs(estimate - left), abs(estimate - up), abs(estimate - up_left)
        if da <= db and da <= dc:
            return left
        if db <= dc:
            return up
        return up_left

    for _ in range(height):
        filter_type = raw[index]
        index += 1
        row = bytearray(raw[index : index + stride])
        index += stride
        for x in range(stride):
            left = row[x - 4] if x >= 4 else 0
            up = prev[x]
            up_left = prev[x - 4] if x >= 4 else 0
            if filter_type == 0:
                value = row[x]
            elif filter_type == 1:
                value = row[x] + left
            elif filter_type == 2:
                value = row[x] + up
            elif filter_type == 3:
                value = row[x] + (left + up) // 2
            elif filter_type == 4:
                value = row[x] + paeth(left, up, up_left)
            else:
                raise SystemExit("未知 PNG 过滤器")
            row[x] = value & 255
        prev = row
        rows.append(row)
    return rows


def write_png(path, width, height, rows):
    def chunk(kind, payload):
        return (
            struct.pack(">I", len(payload))
            + kind
            + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + bytes(row) for row in rows)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    open(path, "wb").write(png)


def outside(pixel):
    red, _green, blue, alpha = pixel
    if alpha == 0:
        return True
    # 圆角外的白底，以及白底和蓝底交界处的浅色抗锯齿。
    return blue - red < 80


def clear_background(rows, width, height):
    seen = bytearray(width * height)
    stack = []
    for x in range(width):
        stack.append((x, 0))
        stack.append((x, height - 1))
    for y in range(height):
        stack.append((0, y))
        stack.append((width - 1, y))
    while stack:
        x, y = stack.pop()
        index = y * width + x
        if seen[index]:
            continue
        seen[index] = 1
        offset = x * 4
        pixel = rows[y][offset : offset + 4]
        if not outside(pixel):
            continue
        rows[y][offset + 3] = 0
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < width and 0 <= ny < height:
                stack.append((nx, ny))


def sample(rows, width, height, x, y):
    if x < 0 or y < 0 or x >= width or y >= height:
        return (0, 0, 0, 0)
    offset = x * 4
    return tuple(rows[y][offset : offset + 4])


def shrink(rows, width, height, scale):
    output = [bytearray(width * 4) for _ in range(height)]
    margin_x = width * (1 - scale) / 2
    margin_y = height * (1 - scale) / 2
    for y in range(height):
        source_y = (y - margin_y) / scale
        y0 = int(source_y)
        fy = source_y - y0
        for x in range(width):
            source_x = (x - margin_x) / scale
            x0 = int(source_x)
            fx = source_x - x0
            samples = (
                sample(rows, width, height, x0, y0),
                sample(rows, width, height, x0 + 1, y0),
                sample(rows, width, height, x0, y0 + 1),
                sample(rows, width, height, x0 + 1, y0 + 1),
            )
            weights = ((1 - fx) * (1 - fy), fx * (1 - fy), (1 - fx) * fy, fx * fy)
            pixel = []
            for channel in range(4):
                value = sum(samples[i][channel] * weights[i] for i in range(4))
                pixel.append(max(0, min(255, int(round(value)))))
            output[y][x * 4 : x * 4 + 4] = bytes(pixel)
    return output


def main():
    if len(sys.argv) != 3:
        raise SystemExit("用法: mac_icon.py 输入.png 输出.png")
    width, height, rows = read_png(sys.argv[1])
    clear_background(rows, width, height)
    rows = shrink(rows, width, height, 0.8)
    write_png(sys.argv[2], width, height, rows)


if __name__ == "__main__":
    main()
