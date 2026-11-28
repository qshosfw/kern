/* SPDX-License-Identifier: GPL-3.0-or-later OR MIT
 *
 * rigwire.js — Universal Zero-Dependency ES5 UMD Reference SDK for RigWire v2,
 *              Ascon-128 AEAD, COBS, CRC-16/32 & Kern Power-of-Two Tables.
 *
 * Targets:
 *   - KaiOS 2.5 (Gecko 48 ES5, zero BigInt!) & KaiOS 3.0
 *   - Web Browsers (WebSerial, WebBluetooth BLE NUS, WebUSB)
 *   - Node.js, Deno, Bun, QuickJS, Duktape, Espruino
 */
(function (root, factory) {
    if (typeof module === "object" && module.exports) {
        module.exports = factory();
    } else if (typeof define === "function" && define.amd) {
        define([], factory);
    } else {
        root.RigWire = factory();
    }
}(typeof self !== "undefined" ? self : this, function () {
    "use strict";

    var WIRE_IDENT = 0x52;
    var COBS_DELIM = 0x00;
    var HEADER_LEN = 8;
    var CRC16_LEN = 2;
    var ASCON_TAG_LEN = 16;

    var FLAG_QOS_CRIT = 0x00;
    var FLAG_QOS_HIGH = 0x01;
    var FLAG_QOS_NORM = 0x02;
    var FLAG_QOS_BULK = 0x03;
    var FLAG_ENC      = 0x04;
    var FLAG_AUTH     = 0x08;
    var FLAG_NO_SEQ   = 0x10;
    var FLAG_ACK_REQ  = 0x20;
    var FLAG_ERR      = 0x40;
    var FLAG_RESP     = 0x80;

    function allocBytes(n) {
        if (typeof Uint8Array !== "undefined") {
            return new Uint8Array(n);
        }
        var a = new Array(n);
        for (var i = 0; i < n; i++) { a[i] = 0; }
        return a;
    }

    function sliceBytes(b, start, end) {
        if (typeof b.subarray === "function") {
            var sub = b.subarray(start, end);
            var out = new Uint8Array(sub.length);
            out.set(sub);
            return out;
        }
        return b.slice(start, end);
    }

    function readU16LE(p, off) {
        return ((p[off] & 0xFF) | ((p[off + 1] & 0xFF) << 8)) >>> 0;
    }

    function writeU16LE(p, off, v) {
        p[off]     = v & 0xFF;
        p[off + 1] = (v >>> 8) & 0xFF;
    }

    function readU32LE(p, off) {
        return ((p[off] & 0xFF) |
               ((p[off + 1] & 0xFF) << 8) |
               ((p[off + 2] & 0xFF) << 16) |
               ((p[off + 3] & 0xFF) << 24)) >>> 0;
    }

    function writeU32LE(p, off, v) {
        p[off]     = v & 0xFF;
        p[off + 1] = (v >>> 8) & 0xFF;
        p[off + 2] = (v >>> 16) & 0xFF;
        p[off + 3] = (v >>> 24) & 0xFF;
    }

    function readU32BE(p, off) {
        return (((p[off] & 0xFF) << 24) |
                ((p[off + 1] & 0xFF) << 16) |
                ((p[off + 2] & 0xFF) << 8) |
                (p[off + 3] & 0xFF)) >>> 0;
    }

    function writeU32BE(p, off, v) {
        p[off]     = (v >>> 24) & 0xFF;
        p[off + 1] = (v >>> 16) & 0xFF;
        p[off + 2] = (v >>> 8) & 0xFF;
        p[off + 3] = v & 0xFF;
    }

    function crc16Ccitt(data, len) {
        var n = (typeof len === "number") ? len : data.length;
        var crc = 0xFFFF;
        for (var i = 0; i < n; i++) {
            crc ^= ((data[i] & 0xFF) << 8);
            for (var b = 0; b < 8; b++) {
                if ((crc & 0x8000) !== 0) {
                    crc = ((crc << 1) ^ 0x1021) & 0xFFFF;
                } else {
                    crc = (crc << 1) & 0xFFFF;
                }
            }
        }
        return crc >>> 0;
    }

    var CRC32_LUT = [
        0x00000000, 0x1DB71064, 0x3B6E20C8, 0x26D930AC,
        0x76DC4190, 0x6B6B51F4, 0x4DB26158, 0x5005713C,
        0xEDB88320, 0xF00F9344, 0xD6D6A3E8, 0xCB61B38C,
        0x9B64C2B0, 0x86D3D2D4, 0xA00AE278, 0xBDBDF21C
    ];

    function crc32(data, len) {
        var n = (typeof len === "number") ? len : data.length;
        var crc = 0xFFFFFFFF;
        for (var i = 0; i < n; i++) {
            crc ^= (data[i] & 0xFF);
            crc = (crc >>> 4) ^ CRC32_LUT[crc & 0x0F];
            crc = (crc >>> 4) ^ CRC32_LUT[crc & 0x0F];
        }
        return (crc ^ 0xFFFFFFFF) >>> 0;
    }

    function cobsEncode(src) {
        var maxLen = src.length + Math.floor(src.length / 254) + 1;
        var dst = allocBytes(maxLen);
        var readIdx = 0, writeIdx = 1, codeIdx = 0, code = 1;
        while (readIdx < src.length) {
            var byteVal = src[readIdx++] & 0xFF;
            if (byteVal === 0) {
                dst[codeIdx] = code;
                codeIdx = writeIdx++;
                code = 1;
            } else {
                dst[writeIdx++] = byteVal;
                code++;
                if (code === 0xFF) {
                    dst[codeIdx] = code;
                    codeIdx = writeIdx++;
                    code = 1;
                }
            }
        }
        dst[codeIdx] = code;
        return sliceBytes(dst, 0, writeIdx);
    }

    function cobsDecode(src) {
        if (src.length === 0) { return allocBytes(0); }
        var dst = allocBytes(src.length);
        var readIdx = 0, writeIdx = 0;
        while (readIdx < src.length) {
            var code = src[readIdx++] & 0xFF;
            if (code === 0 || (readIdx + code - 1) > src.length) {
                throw new Error("RW_ERR_COBS");
            }
            for (var i = 1; i < code; i++) {
                var b = src[readIdx++] & 0xFF;
                if (b === 0) { throw new Error("RW_ERR_COBS"); }
                dst[writeIdx++] = b;
            }
            if (code < 0xFF && readIdx < src.length) {
                dst[writeIdx++] = 0;
            }
        }
        return sliceBytes(dst, 0, writeIdx);
    }

    /* ── Pure 32-Bit Ascon-128 AEAD (Zero BigInt, ES5 JIT Friendly!) ─────── */
    function asconSbox32(w) {
        w[0] ^= w[4]; w[4] ^= w[3]; w[2] ^= w[1];
        var t0 = w[0] ^ ((~w[1]) & w[2]);
        var t1 = w[1] ^ ((~w[2]) & w[3]);
        var t2 = w[2] ^ ((~w[3]) & w[4]);
        var t3 = w[3] ^ ((~w[4]) & w[0]);
        var t4 = w[4] ^ ((~w[0]) & w[1]);
        w[1] = (t1 ^ t0) >>> 0;
        w[0] = (t0 ^ t4) >>> 0;
        w[3] = (t3 ^ t2) >>> 0;
        w[2] = (~t2) >>> 0;
        w[4] = t4 >>> 0;
    }

    var ROT0 = [19, 61, 1, 10, 7];
    var ROT1 = [28, 39, 6, 17, 41];

    function asconPermute(h, l, rounds) {
        var start = 12 - rounds;
        for (var r = start; r < 12; r++) {
            l[2] = (l[2] ^ (0xF0 - r * 0x0F)) >>> 0;
            asconSbox32(h);
            asconSbox32(l);
            for (var w = 0; w < 5; w++) {
                var hw = h[w], lw = l[w];
                var r0 = ROT0[w], a0 = r0 >= 32 ? lw : hw, b0 = r0 >= 32 ? hw : lw, s0 = r0 >= 32 ? r0 - 32 : r0;
                var r0h = ((a0 >>> s0) | (b0 << (32 - s0))) >>> 0;
                var r0l = ((b0 >>> s0) | (a0 << (32 - s0))) >>> 0;

                var r1 = ROT1[w], a1 = r1 >= 32 ? lw : hw, b1 = r1 >= 32 ? hw : lw, s1 = r1 >= 32 ? r1 - 32 : r1;
                var r1h = ((a1 >>> s1) | (b1 << (32 - s1))) >>> 0;
                var r1l = ((b1 >>> s1) | (a1 << (32 - s1))) >>> 0;

                h[w] = (hw ^ r0h ^ r1h) >>> 0;
                l[w] = (lw ^ r0l ^ r1l) >>> 0;
            }
        }
    }

    function ascon128Sponge(key, nonce, ad, data, doCrypt, decrypt) {
        var k0h = readU32BE(key, 0), k0l = readU32BE(key, 4);
        var k1h = readU32BE(key, 8), k1l = readU32BE(key, 12);
        var h = [0x80400C06, k0h, k1h, readU32BE(nonce, 0), readU32BE(nonce, 8)];
        var l = [0x00000000, k0l, k1l, readU32BE(nonce, 4), readU32BE(nonce, 12)];

        asconPermute(h, l, 12);
        h[3] = (h[3] ^ k0h) >>> 0; l[3] = (l[3] ^ k0l) >>> 0;
        h[4] = (h[4] ^ k1h) >>> 0; l[4] = (l[4] ^ k1l) >>> 0;

        var xb = allocBytes(8);
        if (ad && ad.length > 0) {
            var adOff = 0;
            while (adOff + 8 <= ad.length) {
                h[0] = (h[0] ^ readU32BE(ad, adOff)) >>> 0;
                l[0] = (l[0] ^ readU32BE(ad, adOff + 4)) >>> 0;
                asconPermute(h, l, 6);
                adOff += 8;
            }
            writeU32BE(xb, 0, h[0]); writeU32BE(xb, 4, l[0]);
            for (var aj = 0; aj < ad.length - adOff; aj++) {
                xb[aj] ^= ad[adOff + aj];
            }
            xb[ad.length - adOff] ^= 0x80;
            h[0] = readU32BE(xb, 0); l[0] = readU32BE(xb, 4);
            asconPermute(h, l, 6);
        }
        l[4] = (l[4] ^ 1) >>> 0;

        var out = allocBytes(data.length);
        var off = 0;
        for (;;) {
            var rem = data.length - off;
            var n = rem < 8 ? rem : 8;
            writeU32BE(xb, 0, h[0]); writeU32BE(xb, 4, l[0]);
            for (var j = 0; j < n; j++) {
                var b = data[off + j] & 0xFF;
                if (doCrypt) {
                    if (decrypt) {
                        out[off + j] = (xb[j] ^ b) & 0xFF;
                        xb[j] = b;
                    } else {
                        xb[j] = (xb[j] ^ b) & 0xFF;
                        out[off + j] = xb[j];
                    }
                } else {
                    out[off + j] = b;
                    xb[j] = (xb[j] ^ b) & 0xFF;
                }
            }
            if (rem >= 8) {
                h[0] = readU32BE(xb, 0); l[0] = readU32BE(xb, 4);
                asconPermute(h, l, 6);
                off += 8;
            } else {
                xb[n] ^= 0x80;
                h[0] = readU32BE(xb, 0); l[0] = readU32BE(xb, 4);
                break;
            }
        }

        h[1] = (h[1] ^ k0h) >>> 0; l[1] = (l[1] ^ k0l) >>> 0;
        h[2] = (h[2] ^ k1h) >>> 0; l[2] = (l[2] ^ k1l) >>> 0;
        asconPermute(h, l, 12);
        h[3] = (h[3] ^ k0h) >>> 0; l[3] = (l[3] ^ k0l) >>> 0;
        h[4] = (h[4] ^ k1h) >>> 0; l[4] = (l[4] ^ k1l) >>> 0;

        var tag = allocBytes(16);
        writeU32BE(tag, 0, h[3]); writeU32BE(tag, 4, l[3]);
        writeU32BE(tag, 8, h[4]); writeU32BE(tag, 12, l[4]);
        return { data: out, tag: tag };
    }

    function ascon128Encrypt(key, nonce, ad, plaintext) {
        return ascon128Sponge(key, nonce, ad, plaintext, true, false);
    }

    function ascon128Decrypt(key, nonce, ad, ciphertext, expectedTag) {
        var res = ascon128Sponge(key, nonce, ad, ciphertext, true, true);
        var diff = 0;
        for (var i = 0; i < 16; i++) {
            diff |= (res.tag[i] ^ expectedTag[i]);
        }
        if (diff !== 0) {
            for (var z = 0; z < res.data.length; z++) { res.data[z] = 0; }
            throw new Error("RW_ERR_AUTH");
        }
        return res.data;
    }

    function buildImplicitNonce(salt8, direction, epoch24, seqCounter) {
        var n = allocBytes(16);
        for (var i = 0; i < 8; i++) { n[i] = salt8[i] & 0xFF; }
        n[8]  = direction & 0xFF;
        n[9]  = (epoch24 >>> 16) & 0xFF;
        n[10] = (epoch24 >>> 8) & 0xFF;
        n[11] = epoch24 & 0xFF;
        writeU32BE(n, 12, seqCounter >>> 0);
        return n;
    }

    function encodeFrame(flags, channel, op, seq, payload, aeadKey, aeadNonce) {
        var body = payload || allocBytes(0);
        var hasSec = (flags & (FLAG_AUTH | FLAG_ENC)) !== 0;
        var secLen = hasSec ? ASCON_TAG_LEN : 0;
        var raw = allocBytes(HEADER_LEN + body.length + secLen + CRC16_LEN);
        raw[0] = WIRE_IDENT;
        raw[1] = flags & 0xFF;
        raw[2] = channel & 0xFF;
        raw[3] = op & 0xFF;
        writeU16LE(raw, 4, seq);
        writeU16LE(raw, 6, body.length);

        var ad = sliceBytes(raw, 0, HEADER_LEN);
        if ((flags & FLAG_ENC) !== 0) {
            var enc = ascon128Encrypt(aeadKey, aeadNonce, ad, body);
            for (var i = 0; i < body.length; i++) { raw[HEADER_LEN + i] = enc.data[i]; }
            for (var t = 0; t < 16; t++) { raw[HEADER_LEN + body.length + t] = enc.tag[t]; }
        } else if ((flags & FLAG_AUTH) !== 0) {
            var mac = ascon128Sponge(aeadKey, aeadNonce, ad, body, false, false);
            for (var j = 0; j < body.length; j++) { raw[HEADER_LEN + j] = body[j]; }
            for (var mt = 0; mt < 16; mt++) { raw[HEADER_LEN + body.length + mt] = mac.tag[mt]; }
        } else {
            for (var k = 0; k < body.length; k++) { raw[HEADER_LEN + k] = body[k]; }
        }

        var crc = crc16Ccitt(raw, raw.length - CRC16_LEN);
        writeU16LE(raw, raw.length - CRC16_LEN, crc);
        var cobs = cobsEncode(raw);
        var wire = allocBytes(cobs.length + 1);
        for (var w = 0; w < cobs.length; w++) { wire[w] = cobs[w]; }
        wire[cobs.length] = COBS_DELIM;
        return wire;
    }

    function decodeCobsFrame(cobsBody, aeadKey, aeadNonce) {
        var raw = cobsDecode(cobsBody);
        if (raw.length < HEADER_LEN + CRC16_LEN) { throw new Error("RW_ERR_TOO_SHORT"); }
        if (raw[0] !== WIRE_IDENT) { throw new Error("RW_ERR_BAD_IDENT"); }
        var flags = raw[1], channel = raw[2], op = raw[3];
        var seq = readU16LE(raw, 4), len = readU16LE(raw, 6);
        var hasSec = (flags & (FLAG_AUTH | FLAG_ENC)) !== 0;
        var expectedLen = HEADER_LEN + len + (hasSec ? ASCON_TAG_LEN : 0) + CRC16_LEN;
        if (raw.length !== expectedLen) { throw new Error("RW_ERR_BAD_LEN"); }
        var wireCrc = readU16LE(raw, raw.length - CRC16_LEN);
        var calcCrc = crc16Ccitt(raw, raw.length - CRC16_LEN);
        if (wireCrc !== calcCrc) { throw new Error("RW_ERR_BAD_CRC"); }

        var payload = sliceBytes(raw, HEADER_LEN, HEADER_LEN + len);
        if (hasSec) {
            var ad = sliceBytes(raw, 0, HEADER_LEN);
            var tag = sliceBytes(raw, HEADER_LEN + len, HEADER_LEN + len + 16);
            if ((flags & FLAG_ENC) !== 0) {
                payload = ascon128Decrypt(aeadKey, aeadNonce, ad, payload, tag);
            } else {
                var mac = ascon128Sponge(aeadKey, aeadNonce, ad, payload, false, false);
                var diff = 0;
                for (var i = 0; i < 16; i++) { diff |= (mac.tag[i] ^ tag[i]); }
                if (diff !== 0) { throw new Error("RW_ERR_AUTH"); }
            }
        }
        return { flags: flags, channel: channel, op: op, seq: seq, payload: payload };
    }

    function RxCollector(maxLen) {
        this.maxLen = maxLen || 560;
        this.buf = allocBytes(this.maxLen);
        this.len = 0;
        this.overflow = false;
    }

    RxCollector.prototype.pushBytes = function (chunk, onFrame) {
        for (var i = 0; i < chunk.length; i++) {
            var b = chunk[i] & 0xFF;
            if (b === COBS_DELIM) {
                var n = this.len;
                var ovf = this.overflow;
                this.len = 0;
                this.overflow = false;
                if (!ovf && n > 0) {
                    onFrame(sliceBytes(this.buf, 0, n));
                }
            } else if (!this.overflow) {
                if (this.len >= this.maxLen) {
                    this.overflow = true;
                    this.len = 0;
                } else {
                    this.buf[this.len++] = b;
                }
            }
        }
    };

    function packChannelRecord(ch) {
        var out = allocBytes(64);
        writeU32LE(out, 0x00, ch.rxFrequencyHz >>> 0);
        writeU32LE(out, 0x04, ch.txFrequencyHz >>> 0);
        writeU16LE(out, 0x08, ch.stepHzX10 || 0);
        writeU16LE(out, 0x0A, ch.bandwidth50Hz || 0);
        out[0x0C] = (ch.modulation || 0) & 0xFF;
        out[0x0D] = (ch.squelchLevel || 0) & 0xFF;
        writeU16LE(out, 0x0E, ch.rxSubaudible || 0);
        writeU16LE(out, 0x10, ch.txSubaudible || 0);
        writeU16LE(out, 0x12, ch.zoneMask || 0);
        writeU16LE(out, 0x14, ch.flags || 0);
        writeU16LE(out, 0x16, ch.powerMw || 0);
        writeU16LE(out, 0x18, ch.protoRef || 0);
        writeU16LE(out, 0x1A, ch.uid || 0);
        out[0x1C] = (ch.proto || 0) & 0xFF;
        if (ch.name) {
            for (var i = 0; i < 16 && i < ch.name.length; i++) {
                out[0x26 + i] = ch.name.charCodeAt(i) & 0xFF;
            }
        }
        writeU16LE(out, 0x3E, crc16Ccitt(out, 0x3E));
        return out;
    }

    function unpackChannelRecord(raw) {
        if (raw.length < 64) { throw new Error("RW_ERR_TOO_SHORT"); }
        if (crc16Ccitt(raw, 0x3E) !== readU16LE(raw, 0x3E)) {
            throw new Error("RW_ERR_BAD_CRC");
        }
        var nameChars = [];
        for (var i = 0; i < 16; i++) {
            var c = raw[0x26 + i] & 0xFF;
            if (c === 0) { break; }
            nameChars.push(String.fromCharCode(c));
        }
        return {
            rxFrequencyHz: readU32LE(raw, 0x00),
            txFrequencyHz: readU32LE(raw, 0x04),
            stepHzX10: readU16LE(raw, 0x08),
            bandwidth50Hz: readU16LE(raw, 0x0A),
            modulation: raw[0x0C],
            squelchLevel: raw[0x0D],
            rxSubaudible: readU16LE(raw, 0x0E),
            txSubaudible: readU16LE(raw, 0x10),
            zoneMask: readU16LE(raw, 0x12),
            flags: readU16LE(raw, 0x14),
            powerMw: readU16LE(raw, 0x16),
            protoRef: readU16LE(raw, 0x18),
            uid: readU16LE(raw, 0x1A),
            proto: raw[0x1C],
            name: nameChars.join("")
        };
    }

    var AUDIO_HDR_LEN = 14;
    var AUDIO_SQ_OPEN = 0x01;
    var AUDIO_CLIPPED = 0x02;
    var AUDIO_TX_DIR  = 0x04;
    var AUDIO_SILENCE = 0x08;
    var HELLO_FLAG_DRY_TX = 0x01;

    function ulawEncode(pcm) {
        var v = pcm | 0;
        var sign = 0;
        var mag;
        if (v < 0) { sign = 0x80; mag = -v; } else { mag = v; }
        if (mag > 32635) { mag = 32635; }
        mag += 132;
        var exp = 7;
        var mask = 0x4000;
        while (exp > 0 && (mag & mask) === 0) {
            exp--;
            mask >>>= 1;
        }
        var mant = (mag >>> (exp + 3)) & 0x0F;
        return (~(sign | (exp << 4) | mant)) & 0xFF;
    }

    function ulawDecode(ulaw) {
        var u = (~ulaw) & 0xFF;
        var sign = u & 0x80;
        var exp  = (u >>> 4) & 0x07;
        var mant = u & 0x0F;
        var t = (((mant << 3) + 0x84) << exp) - 0x84;
        return sign ? -t : t;
    }

    function pack12(samples) {
        var pairs = samples.length >>> 1;
        var out = allocBytes(pairs * 3);
        var o = 0;
        for (var i = 0; i < pairs * 2; i += 2) {
            var s0 = samples[i] & 0x0FFF;
            var s1 = samples[i + 1] & 0x0FFF;
            out[o++] = s0 & 0xFF;
            out[o++] = ((s0 >>> 8) & 0x0F) | ((s1 & 0x0F) << 4);
            out[o++] = (s1 >>> 4) & 0xFF;
        }
        return out;
    }

    function unpack12(buf) {
        var triples = Math.floor(buf.length / 3);
        var out = new Array(triples * 2);
        var o = 0;
        for (var i = 0; i < triples * 3; i += 3) {
            var b0 = buf[i] & 0xFF;
            var b1 = buf[i + 1] & 0xFF;
            var b2 = buf[i + 2] & 0xFF;
            out[o++] = b0 | ((b1 & 0x0F) << 8);
            out[o++] = (b1 >>> 4) | (b2 << 4);
        }
        return out;
    }

    function packAudioHeader(h) {
        var out = allocBytes(AUDIO_HDR_LEN);
        var c = h.codec || "ULAW";
        out[0] = c.charCodeAt(0) & 0xFF;
        out[1] = c.charCodeAt(1) & 0xFF;
        out[2] = c.charCodeAt(2) & 0xFF;
        out[3] = c.charCodeAt(3) & 0xFF;
        writeU16LE(out, 4, h.rateHz || 12000);
        writeU16LE(out, 6, h.seq || 0);
        writeU32LE(out, 8, h.sampleIdx || 0);
        out[12] = (h.slot || 0) & 0xFF;
        out[13] = (h.flags || 0) & 0xFF;
        return out;
    }

    function unpackAudioHeader(raw) {
        if (raw.length < AUDIO_HDR_LEN) { throw new Error("RW_ERR_TOO_SHORT"); }
        return {
            codec: String.fromCharCode(raw[0], raw[1], raw[2], raw[3]),
            rateHz: readU16LE(raw, 4),
            seq: readU16LE(raw, 6),
            sampleIdx: readU32LE(raw, 8),
            slot: raw[12] & 0xFF,
            flags: raw[13] & 0xFF
        };
    }

    return {
        WIRE_IDENT: WIRE_IDENT,
        FLAG_ENC: FLAG_ENC,
        FLAG_AUTH: FLAG_AUTH,
        FLAG_NO_SEQ: FLAG_NO_SEQ,
        FLAG_ACK_REQ: FLAG_ACK_REQ,
        FLAG_ERR: FLAG_ERR,
        FLAG_RESP: FLAG_RESP,
        FLAG_QOS_CRIT: FLAG_QOS_CRIT,
        FLAG_QOS_HIGH: FLAG_QOS_HIGH,
        FLAG_QOS_NORM: FLAG_QOS_NORM,
        FLAG_QOS_BULK: FLAG_QOS_BULK,
        AUDIO_HDR_LEN: AUDIO_HDR_LEN,
        AUDIO_SQ_OPEN: AUDIO_SQ_OPEN,
        AUDIO_CLIPPED: AUDIO_CLIPPED,
        AUDIO_TX_DIR: AUDIO_TX_DIR,
        AUDIO_SILENCE: AUDIO_SILENCE,
        HELLO_FLAG_DRY_TX: HELLO_FLAG_DRY_TX,
        crc16Ccitt: crc16Ccitt,
        crc32: crc32,
        cobsEncode: cobsEncode,
        cobsDecode: cobsDecode,
        ascon128Encrypt: ascon128Encrypt,
        ascon128Decrypt: ascon128Decrypt,
        buildImplicitNonce: buildImplicitNonce,
        encodeFrame: encodeFrame,
        decodeCobsFrame: decodeCobsFrame,
        RxCollector: RxCollector,
        packChannelRecord: packChannelRecord,
        unpackChannelRecord: unpackChannelRecord,
        ulawEncode: ulawEncode,
        ulawDecode: ulawDecode,
        pack12: pack12,
        unpack12: unpack12,
        packAudioHeader: packAudioHeader,
        unpackAudioHeader: unpackAudioHeader
    };
}));
