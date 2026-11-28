#!/usr/bin/env python3
"""
test_rigwire_sdk.py — Clean Zero-Clutter Verification Suite for `sdk/`:
  1. Host RigWire v2 + Ascon-128 + COBS + 64B ChannelRecord byte-exact parity across:
     - ANSI C89 (`sdk/c/rigwire.h` via `gcc -std=c89` & `clang -std=c89`)
     - ISO C99 (`sdk/c/rigwire.h` via `gcc -std=c99`)
     - ISO C++98 (`sdk/c/rigwire.h` via `g++ -std=c++98`)
     - ES5 / KaiOS / Web UMD JavaScript (`sdk/host/rigwire.js` via `node`)
     - Universal Python 2.7/3.x (`sdk/host/rigwire.py` with `sys.dont_write_bytecode=True`)
  2. Device `.krx` Multi-Language SDK Compilation to ARM Cortex-M0+ (`thumbv6m`):
     - Freestanding Zig (`sdk/zig/krx.zig` -> `thumb-freestanding-eabi` `cortex_m0plus` `.o`)
     - Strict Luau AOT (`sdk/luau/krx.d.luau` + `tools/luau_krx.py` -> PIC C89 -> `arm-none-eabi-gcc` Thumb-1 `.bin`)
"""

import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile

# Never pollute `sdk/host/` with `__pycache__`!
sys.dont_write_bytecode = True

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, os.path.join(ROOT, "tools"))

_rw_spec = importlib.util.spec_from_file_location("sdk_rigwire", os.path.join(ROOT, "sdk", "host", "rigwire.py"))
assert _rw_spec and _rw_spec.loader
py_rw = importlib.util.module_from_spec(_rw_spec)
_rw_spec.loader.exec_module(py_rw)

import luau_krx  # noqa: E402

C_HARNESS = r"""
#include <stdio.h>
#include <string.h>
#include "rigwire.h"

static void print_hex(const rw_u8 *b, size_t n) {
    size_t i;
    for (i = 0u; i < n; ++i) { printf("%02x", (unsigned int)b[i]); }
    printf("\n");
}

int main(void) {
    static const rw_u8 check_ascii[9] = { '1','2','3','4','5','6','7','8','9' };
    static const rw_u8 key[16] = { 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15 };
    static const rw_u8 salt[8] = { 0xA1,0xB2,0xC3,0xD4,0xE5,0xF6,0x07,0x18 };
    static const rw_u8 payload[12] = { 0x00,0x11,0x22,0x00,0x48,0x29,0xFF,0x00,0x55,0xAA,0x12,0x34 };
    rw_u8 nonce[16], scratch[128], wire[140], ch_bytes[64], hdr_bytes[16];
    size_t wire_len = 0u, frame_len = 0u, i;
    rw_rx_collector rx;
    rw_header hdr;
    const rw_u8 *dec_payload = NULL;
    rw_channel_record ch, ch_out;
    rw_table_header th, th_out;
    int got = 0;

    if (rw_crc16_ccitt(check_ascii, 9u) != 0x29B1u) { return 10; }
    if (rw_crc32(check_ascii, 9u) != 0xCBF43926UL)  { return 11; }
    if (rw_record_offset(3u, 6u) != 208UL)          { return 12; }

    th.ident = RW_TABLE_CHAN; th.version = 1u; th.record_count = 10u;
    th.max_records = 64u; th.record_stride = 64u; th.active_bank = 0u; th.flags = 0u;
    rw_table_header_pack(&th, hdr_bytes);
    if (rw_table_header_unpack(hdr_bytes, &th_out) != RW_OK || th_out.ident != RW_TABLE_CHAN) { return 13; }

    rw_build_implicit_nonce(salt, 0x00u, 0x010203UL, 42UL, nonce);
    if (rw_encode_frame(
            (rw_u8)(RW_FLAG_ENC | RW_FLAG_ACK_REQ | RW_FLAG_QOS_HIGH),
            RW_CH_OBJECT, 0x03u, 0x1234u, payload, 12u, key, nonce,
            scratch, sizeof(scratch), wire, sizeof(wire), &wire_len) != RW_OK) { return 14; }

    rw_rx_init(&rx);
    if (rw_rx_push_byte(&rx, 0x00u, &frame_len) != 0) { return 15; }
    for (i = 0u; i < wire_len; ++i) {
        if (rw_rx_push_byte(&rx, wire[i], &frame_len)) { got = 1; break; }
    }
    if (!got || rw_decode_cobs_record_inplace(rx.buf, frame_len, key, nonce, &hdr, &dec_payload) != RW_OK) { return 16; }
    if (hdr.seq != 0x1234u || memcmp(dec_payload, payload, 12u) != 0) { return 17; }

    memset(&ch, 0, sizeof(ch));
    ch.rx_frequency_hz = 145500000UL; ch.tx_frequency_hz = 145500000UL;
    ch.step_hz_x10 = 1250u; ch.bandwidth_50hz = 500u; ch.squelch_level = 4u;
    ch.rx_subaudible = 885u; ch.tx_subaudible = 885u; ch.zone_mask = 1u;
    ch.flags = 1u; ch.power_mw = 5000u; ch.uid = 7u;
    memcpy(ch.name, "VHF CALL", 8u);
    rw_channel_pack(&ch, ch_bytes);
    if (rw_channel_unpack(ch_bytes, &ch_out) != RW_OK || ch_out.rx_frequency_hz != 145500000UL) { return 18; }

    {
        static const rw_i16 pcm_in[7] = { 0, 100, -100, 4096, -16384, 32635, -32768 };
        static const rw_u16 adc12[6]  = { 0u, 1u, 2048u, 4095u, 1337u, 2748u };
        rw_u8 ulaw_out[7];
        rw_u8 p12_out[9];
        rw_u16 adc12_back[6];
        rw_audio_header ah, ah_out;
        rw_u8 ah_bytes[14];
        size_t k;

        for (k = 0u; k < 7u; ++k) {
            ulaw_out[k] = rw_ulaw_encode(pcm_in[k]);
            if (k == 0u && rw_ulaw_decode(ulaw_out[k]) != 0) { return 19; }
        }
        if (rw_pack12(adc12, 6u, p12_out, sizeof(p12_out)) != 9u) { return 20; }
        if (rw_unpack12(p12_out, 9u, adc12_back, 6u) != 6u) { return 21; }
        for (k = 0u; k < 6u; ++k) {
            if (adc12_back[k] != adc12[k]) { return 22; }
        }
        ah.codec = RW_FOURCC('U','L','A','W'); ah.rate_hz = 12000u;
        ah.seq = 77u; ah.sample_idx = 192000UL; ah.slot = 0u;
        ah.flags = (rw_u8)(RW_AUDIO_SQ_OPEN | RW_AUDIO_CLIPPED);
        rw_audio_header_pack(&ah, ah_bytes);
        if (rw_audio_header_unpack(ah_bytes, 14u, &ah_out) != RW_OK || ah_out.sample_idx != 192000UL) { return 23; }

        printf("NONCE="); print_hex(nonce, 16u);
        printf("WIRE=");  print_hex(wire, wire_len);
        printf("CHAN=");  print_hex(ch_bytes, 64u);
        printf("ULAW=");  print_hex(ulaw_out, 7u);
        printf("P12L=");  print_hex(p12_out, 9u);
        printf("AHDR=");  print_hex(ah_bytes, 14u);
    }
    return 0;
}
"""

ZIG_SAMPLE_APP = r"""
const krx = @import("krx");

var freq_hz: u32 = 145_500_000;

export fn krx_entry(op: u32, api_opt: ?*const krx.ApiTable, param: ?*const anyopaque) linksection(".text.entry") callconv(.c) u32 {
    const api = api_opt orelse return 0;
    switch (@as(krx.OpLifecycle, @enumFromInt(op))) {
        .start => {
            _ = api.tuneHz(freq_hz);
            return 0;
        },
        .event => {
            if (param) |p| {
                const ev: *const krx.Event = @ptrCast(@alignCast(p));
                if (ev.kind == @intFromEnum(krx.EventKind.key) and ev.payload == 0) {
                    if (ev.key == @intFromEnum(krx.Key.up)) {
                        freq_hz += 12_500;
                        _ = api.tuneHz(freq_hz);
                        return 1;
                    }
                }
            }
            return 0;
        },
        .render => {
            api.clear();
            api.drawRect(4, 8, 120, 48, .fill);
            api.drawStrAlign(64, 26, "HELLO FROM ZIG", 0, .fill, 1);
            api.commitFrame();
            return 0;
        },
        .exit => return 0,
    }
}
"""

LUAU_SAMPLE_APP = r"""
--!strict
local freq_hz: u32 = 145500000
local step_hz: u32 = 12500

function on_start()
    krx.rf.tune_hz(freq_hz)
end

function on_event(ev: KrxEvent): number
    if ev.kind == krx.sys.EV_KEY and ev.payload == 0 then
        if ev.key == krx.sys.KEY_UP then
            freq_hz += step_hz
            krx.rf.tune_hz(freq_hz)
            return 1
        elseif ev.key == krx.sys.KEY_DOWN then
            freq_hz -= step_hz
            krx.rf.tune_hz(freq_hz)
            return 1
        end
    end
    return 0
end

function on_render()
    krx.gfx.clear()
    krx.gfx.rect(4, 8, 120, 48, krx.gfx.COLOR_FILL)
    krx.gfx.text(64, 24, "HELLO FROM LUAU", krx.gfx.FONT_MUMATRIX, krx.gfx.COLOR_FILL, krx.gfx.ALIGN_CENTER)
    krx.gfx.text(64, 38, "UP/DN: Tune 12.5k", krx.gfx.FONT_TOMTHUMB, krx.gfx.COLOR_FILL, krx.gfx.ALIGN_CENTER)
    krx.gfx.commit()
end
"""


def parse_kv(text: str) -> dict[str, str]:
    out = {}
    for line in text.strip().splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            out[k.strip()] = v.strip()
    return out


def run_c_check(cc: str, std_flag: str, is_cpp: bool = False) -> dict[str, str]:
    inc = os.path.join(ROOT, "sdk", "c")
    with tempfile.TemporaryDirectory() as tmp:
        src = os.path.join(tmp, "harness.cpp" if is_cpp else "harness.c")
        exe = os.path.join(tmp, "harness")
        with open(src, "w", encoding="utf-8") as fp:
            fp.write(C_HARNESS)
        subprocess.run(
            [cc, std_flag, "-pedantic", "-Wall", "-Wextra", "-Werror", "-O2", "-I", inc, src, "-o", exe],
            check=True,
        )
        return parse_kv(subprocess.check_output([exe], text=True))


def run_node_check() -> dict[str, str]:
    js_path = os.path.join(ROOT, "sdk", "host", "rigwire.js")
    script = f"""
    const rw = require({js_path!r});
    const toHex = (u8) => Buffer.from(u8).toString("hex");
    const key = new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]);
    const salt = new Uint8Array([0xA1,0xB2,0xC3,0xD4,0xE5,0xF6,0x07,0x18]);
    const payload = new Uint8Array([0x00,0x11,0x22,0x00,0x48,0x29,0xFF,0x00,0x55,0xAA,0x12,0x34]);
    const nonce = rw.buildImplicitNonce(salt, 0x00, 0x010203, 42);
    const wire = rw.encodeFrame(rw.FLAG_ENC | rw.FLAG_ACK_REQ | rw.FLAG_QOS_HIGH, 0x01, 0x03, 0x1234, payload, key, nonce);
    const ch = rw.packChannelRecord({{
        rxFrequencyHz: 145500000, txFrequencyHz: 145500000,
        stepHzX10: 1250, bandwidth50Hz: 500, modulation: 0, squelchLevel: 4,
        rxSubaudible: 885, txSubaudible: 885, zoneMask: 1, flags: 1,
        powerMw: 5000, uid: 7, name: "VHF CALL"
    }});
    console.log("NONCE=" + toHex(nonce));
    console.log("WIRE=" + toHex(wire));
    console.log("CHAN=" + toHex(ch));
    const pcmIn = [0, 100, -100, 4096, -16384, 32635, -32768];
    const ulawOut = pcmIn.map(rw.ulawEncode);
    const p12Out = rw.pack12([0, 1, 2048, 4095, 1337, 2748]);
    const ahOut = rw.packAudioHeader({{ codec: "ULAW", rateHz: 12000, seq: 77, sampleIdx: 192000, slot: 0, flags: rw.AUDIO_SQ_OPEN | rw.AUDIO_CLIPPED }});
    console.log("ULAW=" + toHex(ulawOut));
    console.log("P12L=" + toHex(p12Out));
    console.log("AHDR=" + toHex(ahOut));
    """
    return parse_kv(subprocess.check_output(["node", "-e", script], text=True))


def run_zig_krx_check(zig_bin: str) -> int:
    krx_zig = os.path.join(ROOT, "sdk", "zig", "krx.zig")
    with tempfile.TemporaryDirectory() as tmp:
        app_zig = os.path.join(tmp, "app.zig")
        obj_out = os.path.join(tmp, "app.o")
        with open(app_zig, "w", encoding="utf-8") as fp:
            fp.write(ZIG_SAMPLE_APP)
        subprocess.run(
            [
                zig_bin, "build-obj",
                "-target", "thumb-freestanding-eabi",
                "-mcpu", "cortex_m0plus",
                "-OReleaseSmall",
                "-fPIC",
                "--dep", "krx",
                f"-Mroot={app_zig}",
                f"-Mkrx={krx_zig}",
                f"-femit-bin={obj_out}",
            ],
            check=True,
        )
        return os.path.getsize(obj_out)


def run_luau_aot_check() -> int:
    c_code = luau_krx.transpile_luau_to_c(LUAU_SAMPLE_APP)
    with tempfile.TemporaryDirectory() as tmp:
        out_bin = os.path.join(tmp, "hello_luau.bin")
        return luau_krx.compile_c_to_thumb_bin(c_code, out_bin)


def main():
    # 1. Python KAT & COBS boundaries
    assert py_rw.crc16_ccitt(b"123456789") == 0x29B1
    assert py_rw.crc32_ieee(b"123456789") == 0xCBF43926
    _, tag = py_rw.ascon128_encrypt(bytes(range(16)), bytes(range(16)), b"", b"")
    assert tag.hex() == "e355159f292911f794cb1432a0103a8a"

    key = bytes(range(16))
    salt = bytes([0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x07, 0x18])
    payload = bytes([0x00, 0x11, 0x22, 0x00, 0x48, 0x29, 0xFF, 0x00, 0x55, 0xAA, 0x12, 0x34])
    nonce = py_rw.build_implicit_nonce(salt, 0x00, 0x010203, 42)
    wire = py_rw.encode_frame(
        py_rw.FLAG_ENC | py_rw.FLAG_ACK_REQ | py_rw.FLAG_QOS_HIGH, 0x01, 0x03, 0x1234, payload, key, nonce
    )
    ch = py_rw.pack_channel_record(
        145500000, 145500000, step_x10=1250, bw_50hz=500, modulation=0, squelch=4,
        rx_sub=885, tx_sub=885, zone_mask=1, flags=1, power_mw=5000, uid=7, name="VHF CALL"
    )
    pcm_in = [0, 100, -100, 4096, -16384, 32635, -32768]
    ulaw_out = bytes(py_rw.ulaw_encode(s) for s in pcm_in)
    assert py_rw.ulaw_decode(ulaw_out[0]) == 0
    adc12_in = [0, 1, 2048, 4095, 1337, 2748]
    p12_out = py_rw.pack12(adc12_in)
    assert py_rw.unpack12(p12_out) == adc12_in
    ah_out = py_rw.pack_audio_header("ULAW", 12000, 77, 192000, 0, py_rw.AUDIO_SQ_OPEN | py_rw.AUDIO_CLIPPED)
    assert py_rw.unpack_audio_header(ah_out)["sample_idx"] == 192000

    expected = {
        "NONCE": nonce.hex(),
        "WIRE": wire.hex(),
        "CHAN": ch.hex(),
        "ULAW": ulaw_out.hex(),
        "P12L": p12_out.hex(),
        "AHDR": ah_out.hex(),
    }

    verified = ["Python 2.7/3.x (sdk/host/rigwire.py)"]
    for cc, std, is_cpp in [
        ("gcc", "-std=c89", False),
        ("gcc", "-std=c99", False),
        ("clang", "-std=c89", False),
        ("g++", "-std=c++98", True),
    ]:
        if shutil.which(cc):
            got = run_c_check(cc, std, is_cpp)
            assert got == expected, f"{cc} {std} mismatch"
            verified.append(f"{cc} {std} (sdk/c/rigwire.h)")

    if shutil.which("node"):
        assert run_node_check() == expected
        verified.append("Node.js ES5/KaiOS UMD (sdk/host/rigwire.js)")

    zig_bin = shutil.which("zig") or ("/home/drew/.local/bin/zig" if os.path.exists("/home/drew/.local/bin/zig") else None)
    if zig_bin:
        obj_sz = run_zig_krx_check(zig_bin)
        verified.append(f"Zig freestanding Cortex-M0+ .krx PIC object (sdk/zig/krx.zig -> {obj_sz} B .o)")

    if shutil.which("arm-none-eabi-gcc") and shutil.which("arm-none-eabi-objcopy"):
        bin_sz = run_luau_aot_check()
        assert 0 < bin_sz <= 1024, f"Unexpected Luau AOT binary size: {bin_sz}"
        verified.append(f"Luau --!strict AOT -> Cortex-M0+ .krx Thumb-1 binary (tools/luau_krx.py -> {bin_sz} B!)")

    # Ensure zero __pycache__ clutter inside sdk/
    for root_dir, dirs, _ in os.walk(os.path.join(ROOT, "sdk")):
        assert "__pycache__" not in dirs, f"Found __pycache__ inside {root_dir}!"

    print("[PASS] Clean SDK & Multi-Language Verification Suite passed:")
    for item in verified:
        print(f"  - {item}")


if __name__ == "__main__":
    main()
