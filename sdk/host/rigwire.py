#!/usr/bin/env python
# -*- coding: utf-8 -*-
# SPDX-License-Identifier: GPL-3.0-or-later OR MIT
"""
rigwire_core.py — Universal Zero-Dependency Python 2.7 / 3.x Reference SDK
                  for RigWire v2, Ascon-128 AEAD, COBS, CRC-16/32 & Kern Tables.

Portability:
  - Runs unchanged on Python 2.7 (Nokia N900 Maemo 5, legacy Unix) AND Python 3.6..3.14
    (Linux, XNU/macOS, Windows, BSD, Android Termux) with ZERO third-party packages.
"""

import struct

WIRE_IDENT    = 0x52
COBS_DELIM    = 0x00
HEADER_LEN    = 8
CRC16_LEN     = 2
ASCON_TAG_LEN = 16

FLAG_QOS_CRIT = 0x00
FLAG_QOS_HIGH = 0x01
FLAG_QOS_NORM = 0x02
FLAG_QOS_BULK = 0x03
FLAG_ENC      = 0x04
FLAG_AUTH     = 0x08
FLAG_NO_SEQ   = 0x10
FLAG_ACK_REQ  = 0x20
FLAG_ERR      = 0x40
FLAG_RESP     = 0x80

_CRC32_LUT = (
    0x00000000, 0x1DB71064, 0x3B6E20C8, 0x26D930AC,
    0x76DC4190, 0x6B6B51F4, 0x4DB26158, 0x5005713C,
    0xEDB88320, 0xF00F9344, 0xD6D6A3E8, 0xCB61B38C,
    0x9B64C2B0, 0x86D3D2D4, 0xA00AE278, 0xBDBDF21C,
)

_ROT = ((19, 28), (61, 39), (1, 6), (10, 17), (7, 41))


def _to_bytearray(data):
    if isinstance(data, bytearray):
        return data
    return bytearray(data)


def crc16_ccitt(data):
    b = _to_bytearray(data)
    crc = 0xFFFF
    for byte in b:
        crc ^= (byte << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc


def crc32_ieee(data):
    b = _to_bytearray(data)
    crc = 0xFFFFFFFF
    for byte in b:
        crc ^= byte
        crc = (crc >> 4) ^ _CRC32_LUT[crc & 0x0F]
        crc = (crc >> 4) ^ _CRC32_LUT[crc & 0x0F]
    return (crc ^ 0xFFFFFFFF) & 0xFFFFFFFF


def cobs_encode(src):
    b = _to_bytearray(src)
    dst = bytearray(len(b) + (len(b) // 254) + 1)
    read_idx = 0
    write_idx = 1
    code_idx = 0
    code = 1
    while read_idx < len(b):
        val = b[read_idx]
        read_idx += 1
        if val == 0:
            dst[code_idx] = code
            code_idx = write_idx
            write_idx += 1
            code = 1
        else:
            dst[write_idx] = val
            write_idx += 1
            code += 1
            if code == 0xFF:
                dst[code_idx] = code
                code_idx = write_idx
                write_idx += 1
                code = 1
    dst[code_idx] = code
    return bytes(dst[:write_idx])


def cobs_decode(src):
    b = _to_bytearray(src)
    if not b:
        return b""
    out = bytearray()
    read_idx = 0
    n = len(b)
    while read_idx < n:
        code = b[read_idx]
        read_idx += 1
        if code == 0 or (read_idx + code - 1) > n:
            raise ValueError("RW_ERR_COBS")
        for _ in range(code - 1):
            v = b[read_idx]
            read_idx += 1
            if v == 0:
                raise ValueError("RW_ERR_COBS")
            out.append(v)
        if code < 0xFF and read_idx < n:
            out.append(0)
    return bytes(out)


def _sbox32(w):
    w[0] ^= w[4]
    w[4] ^= w[3]
    w[2] ^= w[1]
    t0 = (w[0] ^ ((~w[1]) & w[2])) & 0xFFFFFFFF
    t1 = (w[1] ^ ((~w[2]) & w[3])) & 0xFFFFFFFF
    t2 = (w[2] ^ ((~w[3]) & w[4])) & 0xFFFFFFFF
    t3 = (w[3] ^ ((~w[4]) & w[0])) & 0xFFFFFFFF
    t4 = (w[4] ^ ((~w[0]) & w[1])) & 0xFFFFFFFF
    w[1] = (t1 ^ t0) & 0xFFFFFFFF
    w[0] = (t0 ^ t4) & 0xFFFFFFFF
    w[3] = (t3 ^ t2) & 0xFFFFFFFF
    w[2] = (~t2) & 0xFFFFFFFF
    w[4] = t4


def _rotr_pair(hi, lo, r):
    if r >= 32:
        a, b, s = lo, hi, r - 32
    else:
        a, b, s = hi, lo, r
    if s == 0:
        return a, b
    inv = 32 - s
    return (((a >> s) | (b << inv)) & 0xFFFFFFFF,
            ((b >> s) | (a << inv)) & 0xFFFFFFFF)


def _ascon_permute(h, l, rounds):
    start = 12 - rounds
    for r in range(start, 12):
        l[2] = (l[2] ^ (0xF0 - r * 0x0F)) & 0xFFFFFFFF
        _sbox32(h)
        _sbox32(l)
        for w in range(5):
            r0h, r0l = _rotr_pair(h[w], l[w], _ROT[w][0])
            r1h, r1l = _rotr_pair(h[w], l[w], _ROT[w][1])
            h[w] = (h[w] ^ r0h ^ r1h) & 0xFFFFFFFF
            l[w] = (l[w] ^ r0l ^ r1l) & 0xFFFFFFFF


def ascon128_sponge(key, nonce, ad, data, do_crypt=True, decrypt=False):
    kb = bytes(_to_bytearray(key))
    nb = bytes(_to_bytearray(nonce))
    adb = _to_bytearray(ad)
    db = _to_bytearray(data)

    k0h, k0l, k1h, k1l = struct.unpack(">IIII", kb)
    n0h, n0l, n1h, n1l = struct.unpack(">IIII", nb)

    h = [0x80400C06, k0h, k1h, n0h, n1h]
    l = [0x00000000, k0l, k1l, n0l, n1l]
    _ascon_permute(h, l, 12)
    h[3] ^= k0h; l[3] ^= k0l
    h[4] ^= k1h; l[4] ^= k1l

    if len(adb) > 0:
        off = 0
        while off + 8 <= len(adb):
            ah, al = struct.unpack(">II", bytes(adb[off:off + 8]))
            h[0] ^= ah; l[0] ^= al
            _ascon_permute(h, l, 6)
            off += 8
        xb = bytearray(struct.pack(">II", h[0], l[0]))
        rem = len(adb) - off
        for j in range(rem):
            xb[j] ^= adb[off + j]
        xb[rem] ^= 0x80
        h[0], l[0] = struct.unpack(">II", bytes(xb))
        _ascon_permute(h, l, 6)

    l[4] ^= 1
    out = bytearray(len(db))
    off = 0
    while True:
        rem = len(db) - off
        n = 8 if rem >= 8 else rem
        xb = bytearray(struct.pack(">II", h[0], l[0]))
        for j in range(n):
            b = db[off + j]
            if do_crypt:
                if decrypt:
                    out[off + j] = xb[j] ^ b
                    xb[j] = b
                else:
                    xb[j] ^= b
                    out[off + j] = xb[j]
            else:
                out[off + j] = b
                xb[j] ^= b
        if rem >= 8:
            h[0], l[0] = struct.unpack(">II", bytes(xb))
            _ascon_permute(h, l, 6)
            off += 8
        else:
            xb[n] ^= 0x80
            h[0], l[0] = struct.unpack(">II", bytes(xb))
            break

    h[1] ^= k0h; l[1] ^= k0l
    h[2] ^= k1h; l[2] ^= k1l
    _ascon_permute(h, l, 12)
    h[3] ^= k0h; l[3] ^= k0l
    h[4] ^= k1h; l[4] ^= k1l
    tag = struct.pack(">IIII", h[3], l[3], h[4], l[4])
    return bytes(out), tag


def ascon128_encrypt(key, nonce, ad, plaintext):
    return ascon128_sponge(key, nonce, ad, plaintext, True, False)


def ascon128_decrypt(key, nonce, ad, ciphertext, expected_tag):
    pt, tag = ascon128_sponge(key, nonce, ad, ciphertext, True, True)
    tb1 = _to_bytearray(tag)
    tb2 = _to_bytearray(expected_tag)
    diff = 0
    for i in range(16):
        diff |= (tb1[i] ^ tb2[i])
    if diff != 0:
        raise ValueError("RW_ERR_AUTH")
    return pt


def build_implicit_nonce(salt8, direction, epoch24, seq_counter):
    s = bytes(_to_bytearray(salt8))
    if len(s) != 8:
        raise ValueError("salt8 must be 8 bytes")
    return s + struct.pack(
        ">BBBB I",
        direction & 0xFF,
        (epoch24 >> 16) & 0xFF,
        (epoch24 >> 8) & 0xFF,
        epoch24 & 0xFF,
        seq_counter & 0xFFFFFFFF,
    )


def encode_frame(flags, channel, op, seq, payload=b"", aead_key=None, aead_nonce=None):
    body = bytes(_to_bytearray(payload))
    hdr = struct.pack("<BBBBHH", WIRE_IDENT, flags & 0xFF, channel & 0xFF, op & 0xFF, seq & 0xFFFF, len(body))
    has_sec = (flags & (FLAG_AUTH | FLAG_ENC)) != 0
    if has_sec:
        if aead_key is None or aead_nonce is None:
            raise ValueError("RW_ERR_AUTH")
        if flags & FLAG_ENC:
            body, tag = ascon128_encrypt(aead_key, aead_nonce, hdr, body)
        else:
            _, tag = ascon128_sponge(aead_key, aead_nonce, hdr, body, False, False)
        raw_wo_crc = hdr + body + tag
    else:
        raw_wo_crc = hdr + body
    crc = crc16_ccitt(raw_wo_crc)
    raw = raw_wo_crc + struct.pack("<H", crc)
    return cobs_encode(raw) + b"\x00"


def decode_cobs_frame(cobs_body, aead_key=None, aead_nonce=None):
    raw = cobs_decode(cobs_body)
    if len(raw) < HEADER_LEN + CRC16_LEN:
        raise ValueError("RW_ERR_TOO_SHORT")
    ident, flags, channel, op, seq, length = struct.unpack("<BBBBHH", raw[:HEADER_LEN])
    if ident != WIRE_IDENT:
        raise ValueError("RW_ERR_BAD_IDENT")
    has_sec = (flags & (FLAG_AUTH | FLAG_ENC)) != 0
    expected = HEADER_LEN + length + (ASCON_TAG_LEN if has_sec else 0) + CRC16_LEN
    if len(raw) != expected:
        raise ValueError("RW_ERR_BAD_LEN")
    wire_crc, = struct.unpack("<H", raw[-2:])
    if wire_crc != crc16_ccitt(raw[:-2]):
        raise ValueError("RW_ERR_BAD_CRC")
    payload = raw[HEADER_LEN:HEADER_LEN + length]
    if has_sec:
        tag = raw[HEADER_LEN + length:HEADER_LEN + length + ASCON_TAG_LEN]
        if flags & FLAG_ENC:
            payload = ascon128_decrypt(aead_key, aead_nonce, raw[:HEADER_LEN], payload, tag)
        else:
            _, calc = ascon128_sponge(aead_key, aead_nonce, raw[:HEADER_LEN], payload, False, False)
            if calc != tag:
                raise ValueError("RW_ERR_AUTH")
    return {
        "flags": flags,
        "channel": channel,
        "op": op,
        "seq": seq,
        "payload": payload,
    }


class RxCollector(object):
    def __init__(self, max_len=560):
        self.max_len = max_len
        self.buf = bytearray()
        self.overflow = False

    def push_bytes(self, chunk):
        frames = []
        for b in _to_bytearray(chunk):
            if b == COBS_DELIM:
                if not self.overflow and len(self.buf) > 0:
                    frames.append(bytes(self.buf))
                self.buf = bytearray()
                self.overflow = False
            elif not self.overflow:
                if len(self.buf) >= self.max_len:
                    self.overflow = True
                    self.buf = bytearray()
                else:
                    self.buf.append(b)
        return frames


def pack_channel_record(
    rx_hz, tx_hz, step_x10=1250, bw_50hz=500, modulation=0, squelch=4,
    rx_sub=0, tx_sub=0, zone_mask=1, flags=0, power_mw=5000,
    proto_ref=0, uid=1, proto=0, audio_block=b"\x00" * 9,
    name="", user_tag=b"\x00" * 8
):
    name_bytes = name.encode("utf-8")[:16] if not isinstance(name, bytes) else name[:16]
    name_padded = name_bytes + (b"\x00" * (16 - len(name_bytes)))
    body = struct.pack(
        "<IIHHBBHHHHHHHB9s16s8s",
        rx_hz, tx_hz, step_x10, bw_50hz, modulation, squelch,
        rx_sub, tx_sub, zone_mask, flags, power_mw, proto_ref, uid, proto,
        bytes(_to_bytearray(audio_block))[:9],
        name_padded,
        bytes(_to_bytearray(user_tag))[:8],
    )
    crc = crc16_ccitt(body)
    return body + struct.pack("<H", crc)


def unpack_channel_record(raw64):
    b = bytes(_to_bytearray(raw64))
    if len(b) != 64:
        raise ValueError("RW_ERR_BAD_LEN")
    stored_crc, = struct.unpack("<H", b[0x3E:0x40])
    if stored_crc != crc16_ccitt(b[:0x3E]):
        raise ValueError("RW_ERR_BAD_CRC")
    fields = struct.unpack("<IIHHBBHHHHHHHB9s16s8s", b[:0x3E])
    raw_name = fields[15].split(b"\x00", 1)[0]
    return {
        "rx_frequency_hz": fields[0],
        "tx_frequency_hz": fields[1],
        "step_hz_x10": fields[2],
        "bandwidth_50hz": fields[3],
        "modulation": fields[4],
        "squelch_level": fields[5],
        "rx_subaudible": fields[6],
        "tx_subaudible": fields[7],
        "zone_mask": fields[8],
        "flags": fields[9],
        "power_mw": fields[10],
        "proto_ref": fields[11],
        "uid": fields[12],
        "proto": fields[13],
        "audio_block": fields[14],
        "name": raw_name.decode("utf-8", "replace"),
        "user_tag": fields[16],
        "record_crc": stored_crc,
    }


AUDIO_HDR_LEN = 14
AUDIO_SQ_OPEN = 0x01
AUDIO_CLIPPED = 0x02
AUDIO_TX_DIR  = 0x04
AUDIO_SILENCE = 0x08
HELLO_FLAG_DRY_TX = 0x01


def ulaw_encode(pcm):
    v = int(pcm)
    if v < 0:
        sign = 0x80
        mag = -v
    else:
        sign = 0
        mag = v
    if mag > 32635:
        mag = 32635
    mag += 132
    exp = 7
    mask = 0x4000
    while exp > 0 and (mag & mask) == 0:
        exp -= 1
        mask >>= 1
    mant = (mag >> (exp + 3)) & 0x0F
    return (~(sign | (exp << 4) | mant)) & 0xFF


def ulaw_decode(ulaw):
    u = (~int(ulaw)) & 0xFF
    sign = u & 0x80
    exp = (u >> 4) & 0x07
    mant = u & 0x0F
    t = (((mant << 3) + 0x84) << exp) - 0x84
    return -t if sign else t


def pack12(samples):
    pairs = len(samples) // 2
    out = bytearray(pairs * 3)
    o = 0
    for i in range(0, pairs * 2, 2):
        s0 = int(samples[i]) & 0x0FFF
        s1 = int(samples[i + 1]) & 0x0FFF
        out[o] = s0 & 0xFF
        out[o + 1] = ((s0 >> 8) & 0x0F) | ((s1 & 0x0F) << 4)
        out[o + 2] = (s1 >> 4) & 0xFF
        o += 3
    return bytes(out)


def unpack12(buf):
    b = _to_bytearray(buf)
    triples = len(b) // 3
    out = []
    for i in range(0, triples * 3, 3):
        b0, b1, b2 = b[i], b[i + 1], b[i + 2]
        out.append(b0 | ((b1 & 0x0F) << 8))
        out.append((b1 >> 4) | (b2 << 4))
    return out


def pack_audio_header(codec, rate_hz, seq, sample_idx, slot=0, flags=0):
    cb = codec.encode("ascii")[:4] if not isinstance(codec, bytes) else codec[:4]
    return struct.pack(
        "<4sHHIBB",
        cb, rate_hz & 0xFFFF, seq & 0xFFFF,
        sample_idx & 0xFFFFFFFF, slot & 0xFF, flags & 0xFF,
    )


def unpack_audio_header(raw):
    b = bytes(_to_bytearray(raw))
    if len(b) < AUDIO_HDR_LEN:
        raise ValueError("RW_ERR_TOO_SHORT")
    codec, rate_hz, seq, sample_idx, slot, flags = struct.unpack("<4sHHIBB", b[:AUDIO_HDR_LEN])
    return {
        "codec": codec.decode("ascii", "replace"),
        "rate_hz": rate_hz,
        "seq": seq,
        "sample_idx": sample_idx,
        "slot": slot,
        "flags": flags,
    }
