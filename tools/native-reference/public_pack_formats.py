"""Bounded independent readers for public-reference data; never loads model code."""

from __future__ import annotations

import json
import math
import struct
from dataclasses import dataclass
from typing import BinaryIO

import numpy as np


class Rejected(ValueError):
    """A declared reference input or operation is outside the fixed contract."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Rejected(message)


def distinct_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def strict_json(raw: bytes) -> object:
    def invalid_constant(_value: str) -> None:
        raise Rejected("nonfinite JSON number")

    def finite_float(value: str) -> float:
        parsed = float(value)
        require(math.isfinite(parsed), "overflowing JSON number")
        return parsed

    try:
        return json.loads(
            raw.decode("utf-8", errors="strict"),
            object_pairs_hook=distinct_object,
            parse_constant=invalid_constant,
            parse_float=finite_float,
        )
    except (ValueError, UnicodeError, RecursionError) as error:
        raise Rejected("invalid bounded JSON") from error


def integer(value: object, maximum: int) -> int:
    require(type(value) is int and 0 <= value <= maximum, "invalid integer extent")
    return value


@dataclass(frozen=True)
class Tensor:
    dtype: str
    shape: tuple[int, ...]
    begin: int
    end: int


WIDTHS = {
    "BOOL": 1, "U8": 1, "I8": 1, "U16": 2, "I16": 2, "F16": 2,
    "BF16": 2, "U32": 4, "I32": 4, "F32": 4, "U64": 8, "I64": 8, "F64": 8,
}
NUMPY_TYPES = {"U32": "<u4", "F16": "<f2", "F32": "<f4"}
HEADER_LIMIT = 4 * 1024 * 1024
TENSOR_LIMIT = 8192


def safetensors_header(handle: BinaryIO, file_bytes: int) -> tuple[dict[str, Tensor], bytes]:
    """Read only a bounded header; reject duplicate names, holes and aliasing."""
    handle.seek(0)
    prefix = handle.read(8)
    require(len(prefix) == 8, "truncated safetensors prefix")
    length = struct.unpack("<Q", prefix)[0]
    require(2 <= length <= HEADER_LIMIT and length + 8 <= file_bytes,
            "safetensors header exceeds its bound")
    raw = handle.read(length)
    require(len(raw) == length, "truncated safetensors header")
    value = strict_json(raw)
    require(type(value) is dict and len(value) <= TENSOR_LIMIT + 1,
            "invalid safetensors root/inventory")
    metadata = value.pop("__metadata__", {})
    require(type(metadata) is dict and len(metadata) <= 64 and all(
        type(key) is str and type(item) is str and len(key) <= 512 and len(item) <= 4096
        for key, item in metadata.items()), "invalid safetensors metadata")
    data_begin = 8 + length
    payload_bytes = file_bytes - data_begin
    tensors = {}
    extents = []
    for name, record in value.items():
        require(type(name) is str and 0 < len(name) <= 512 and "\0" not in name,
                "invalid safetensors tensor name")
        require(type(record) is dict and set(record) == {"dtype", "shape", "data_offsets"},
                "unsupported safetensors descriptor")
        dtype, shape, offsets = record["dtype"], record["shape"], record["data_offsets"]
        require(type(dtype) is str and dtype in WIDTHS, "unsupported safetensors dtype")
        require(type(shape) is list and len(shape) <= 8, "invalid safetensors rank")
        dims = tuple(integer(dimension, 1_000_000_000) for dimension in shape)
        require(type(offsets) is list and len(offsets) == 2, "invalid tensor offsets")
        begin, end = (integer(offset, payload_bytes) for offset in offsets)
        require(begin <= end and end - begin == math.prod(dims) * WIDTHS[dtype],
                "tensor shape/storage extent differs")
        tensors[name] = Tensor(dtype, dims, data_begin + begin, data_begin + end)
        if begin != end:
            extents.append((begin, end))
    cursor = 0
    for begin, end in sorted(extents):
        require(begin == cursor, "safetensors payload hole or overlap")
        cursor = end
    require(cursor == payload_bytes and tensors, "unindexed safetensors payload")
    return tensors, raw


def tensor_array(mapping, descriptor: Tensor, shape: tuple[int, ...], dtypes: tuple[str, ...]) -> np.ndarray:
    require(descriptor.shape == shape and descriptor.dtype in dtypes,
            "reference tensor has unsupported shape/dtype")
    require(descriptor.dtype in NUMPY_TYPES, "reference tensor dtype cannot be decoded")
    # Return owned storage; no NumPy view may outlive the retained mmap owner.
    return np.frombuffer(mapping, dtype=NUMPY_TYPES[descriptor.dtype],
                         count=math.prod(shape), offset=descriptor.begin).reshape(shape).copy()


class GgufReader:
    """Narrow independent reader for the immutable F32 approved adapter."""

    def __init__(self, raw: bytes):
        self.raw, self.position = raw, 0

    def take(self, count: int) -> bytes:
        require(0 <= count <= len(self.raw) - self.position, "truncated GGUF")
        begin = self.position
        self.position += count
        return self.raw[begin:self.position]

    def number(self, fmt: str):
        return struct.unpack("<" + fmt, self.take(struct.calcsize("<" + fmt)))[0]

    def count(self, maximum: int) -> int:
        return integer(self.number("Q"), maximum)

    def text(self) -> str:
        try:
            return self.take(self.count(1_048_576)).decode("utf-8", errors="strict")
        except UnicodeError as error:
            raise Rejected("invalid GGUF text") from error

    def value(self, kind: int, depth: int = 0):
        require(depth <= 2, "GGUF metadata nesting exceeds bound")
        formats = {0: "B", 1: "b", 2: "H", 3: "h", 4: "I", 5: "i", 6: "f",
                   10: "Q", 11: "q", 12: "d"}
        if kind in formats:
            return self.number(formats[kind])
        if kind == 7:
            value = self.number("B")
            require(value in (0, 1), "invalid GGUF bool")
            return bool(value)
        if kind == 8:
            return self.text()
        if kind == 9:
            element = self.number("I")
            return [self.value(element, depth + 1) for _ in range(self.count(65536))]
        raise Rejected("unsupported GGUF metadata type")


def gguf_factors(raw: bytes) -> dict[str, np.ndarray]:
    reader = GgufReader(raw)
    require(reader.take(4) == b"GGUF" and reader.number("I") == 3, "adapter is not GGUF v3")
    count, metadata_count = reader.count(258), reader.count(128)
    require(count == 258, "approved adapter needs exactly 258 factors")
    metadata = {}
    for _ in range(metadata_count):
        key = reader.text()
        require(key not in metadata, "duplicate GGUF metadata")
        metadata[key] = reader.value(reader.number("I"))
    require(metadata.get("general.alignment", 32) == 32, "unsupported GGUF alignment")
    require(metadata.get("general.architecture") == "qwen35" and
            metadata.get("general.type") == "adapter" and metadata.get("adapter.type") == "lora"
            and metadata.get("adapter.lora_alpha") == 1.0, "adapter metadata differs")
    descriptors = {}
    for _ in range(count):
        name = reader.text()
        require(name not in descriptors and len(name) <= 512, "duplicate/oversized GGUF name")
        rank = reader.number("I")
        require(rank == 2, "approved factors must be matrices")
        shape = (reader.count(248320), reader.count(248320))
        require(all(shape) and reader.number("I") == 0, "approved factors must be F32")
        offset = reader.count(len(raw))
        require(offset % 32 == 0, "misaligned GGUF factor")
        descriptors[name] = (shape, offset)
    data_begin = (reader.position + 31) // 32 * 32
    factors, extents = {}, []
    for name, (shape, offset) in descriptors.items():
        begin = data_begin + offset
        end = begin + math.prod(shape) * 4
        require(end <= len(raw), "GGUF factor extends beyond file")
        factors[name] = np.frombuffer(raw, dtype="<f4", count=math.prod(shape), offset=begin).copy()
        require(np.isfinite(factors[name]).all(), "nonfinite adapter factor")
        extents.append((begin, end))
    extents.sort()
    require(all(left[1] <= right[0] for left, right in zip(extents, extents[1:])),
            "GGUF factors overlap")
    expected = {"token_embd.weight": (1, 248320)}
    for block in range(64):
        expected[f"blk.{block}.ffn_down.weight"] = (17408, 1)
        family = "attn_output" if (block + 1) % 4 == 0 else "ssm_out"
        expected[f"blk.{block}.{family}.weight"] = (6144, 1)
    require(set(factors) == {site + suffix for site in expected
                            for suffix in (".lora_a", ".lora_b")}, "adapter writer inventory differs")
    for site, shape in expected.items():
        require(descriptors[site + ".lora_a"][0] == shape and
                descriptors[site + ".lora_b"][0] == (1, 5120), "adapter writer shape differs")
    return factors
