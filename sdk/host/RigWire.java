/* SPDX-License-Identifier: GPL-3.0-or-later OR MIT
 *
 * RigWire.java — Universal Zero-Dependency Java Reference SDK for RigWire v2,
 *                Ascon-128 AEAD, COBS, CRC-16/32 & Kern Power-of-Two Tables.
 *
 * Compatibility:
 *   - J2ME CLDC 1.1 / MIDP 2.0 (Symbian S60, Series 40, classic Nokia handhelds)
 *   - Java 1.4 .. Java 23 (Desktop Windows, Linux, XNU/macOS, BSD)
 *   - Android API 1 .. API 35 (UsbManager CDC-ACM, BluetoothSocket SPP, BLE)
 *   - Uses ONLY java.lang primitives and byte[] (zero java.nio / Java 8 APIs).
 */
package org.kern.rigwire;

public final class RigWire {
    public static final int WIRE_IDENT    = 0x52;
    public static final int COBS_DELIM    = 0x00;
    public static final int HEADER_LEN    = 8;
    public static final int CRC16_LEN     = 2;
    public static final int ASCON_TAG_LEN = 16;

    public static final int FLAG_QOS_CRIT = 0x00;
    public static final int FLAG_QOS_HIGH = 0x01;
    public static final int FLAG_QOS_NORM = 0x02;
    public static final int FLAG_QOS_BULK = 0x03;
    public static final int FLAG_ENC      = 0x04;
    public static final int FLAG_AUTH     = 0x08;
    public static final int FLAG_NO_SEQ   = 0x10;
    public static final int FLAG_ACK_REQ  = 0x20;
    public static final int FLAG_ERR      = 0x40;
    public static final int FLAG_RESP     = 0x80;

    private static final int[] CRC32_LUT = {
        0x00000000, 0x1DB71064, 0x3B6E20C8, 0x26D930AC,
        0x76DC4190, 0x6B6B51F4, 0x4DB26158, 0x5005713C,
        0xEDB88320, 0xF00F9344, 0xD6D6A3E8, 0xCB61B38C,
        0x9B64C2B0, 0x86D3D2D4, 0xA00AE278, 0xBDBDF21C
    };

    private static final int[] ROT0 = { 19, 61, 1, 10, 7 };
    private static final int[] ROT1 = { 28, 39, 6, 17, 41 };

    private RigWire() {}

    public static int readU16LE(byte[] p, int off) {
        return (p[off] & 0xFF) | ((p[off + 1] & 0xFF) << 8);
    }

    public static void writeU16LE(byte[] p, int off, int v) {
        p[off]     = (byte)(v & 0xFF);
        p[off + 1] = (byte)((v >>> 8) & 0xFF);
    }

    public static int readU32LE(byte[] p, int off) {
        return (p[off] & 0xFF) |
               ((p[off + 1] & 0xFF) << 8) |
               ((p[off + 2] & 0xFF) << 16) |
               ((p[off + 3] & 0xFF) << 24);
    }

    public static void writeU32LE(byte[] p, int off, int v) {
        p[off]     = (byte)(v & 0xFF);
        p[off + 1] = (byte)((v >>> 8) & 0xFF);
        p[off + 2] = (byte)((v >>> 16) & 0xFF);
        p[off + 3] = (byte)((v >>> 24) & 0xFF);
    }

    public static int readU32BE(byte[] p, int off) {
        return ((p[off] & 0xFF) << 24) |
               ((p[off + 1] & 0xFF) << 16) |
               ((p[off + 2] & 0xFF) << 8) |
               (p[off + 3] & 0xFF);
    }

    public static void writeU32BE(byte[] p, int off, int v) {
        p[off]     = (byte)((v >>> 24) & 0xFF);
        p[off + 1] = (byte)((v >>> 16) & 0xFF);
        p[off + 2] = (byte)((v >>> 8) & 0xFF);
        p[off + 3] = (byte)(v & 0xFF);
    }

    public static int crc16Ccitt(byte[] data, int off, int len) {
        int crc = 0xFFFF;
        for (int i = 0; i < len; i++) {
            crc ^= ((data[off + i] & 0xFF) << 8);
            for (int b = 0; b < 8; b++) {
                if ((crc & 0x8000) != 0) {
                    crc = ((crc << 1) ^ 0x1021) & 0xFFFF;
                } else {
                    crc = (crc << 1) & 0xFFFF;
                }
            }
        }
        return crc;
    }

    public static int crc32(byte[] data, int off, int len) {
        int crc = 0xFFFFFFFF;
        for (int i = 0; i < len; i++) {
            crc ^= (data[off + i] & 0xFF);
            crc = (crc >>> 4) ^ CRC32_LUT[crc & 0x0F];
            crc = (crc >>> 4) ^ CRC32_LUT[crc & 0x0F];
        }
        return ~crc;
    }

    public static byte[] cobsEncode(byte[] src) {
        byte[] tmp = new byte[src.length + (src.length / 254) + 1];
        int readIdx = 0, writeIdx = 1, codeIdx = 0, code = 1;
        while (readIdx < src.length) {
            int b = src[readIdx++] & 0xFF;
            if (b == 0) {
                tmp[codeIdx] = (byte)code;
                codeIdx = writeIdx++;
                code = 1;
            } else {
                tmp[writeIdx++] = (byte)b;
                code++;
                if (code == 0xFF) {
                    tmp[codeIdx] = (byte)code;
                    codeIdx = writeIdx++;
                    code = 1;
                }
            }
        }
        tmp[codeIdx] = (byte)code;
        byte[] out = new byte[writeIdx];
        System.arraycopy(tmp, 0, out, 0, writeIdx);
        return out;
    }

    public static byte[] cobsDecode(byte[] src, int off, int len) {
        if (len == 0) { return new byte[0]; }
        byte[] tmp = new byte[len];
        int readIdx = off, end = off + len, writeIdx = 0;
        while (readIdx < end) {
            int code = src[readIdx++] & 0xFF;
            if (code == 0 || readIdx + code - 1 > end) {
                throw new IllegalArgumentException("RW_ERR_COBS");
            }
            for (int i = 1; i < code; i++) {
                int b = src[readIdx++] & 0xFF;
                if (b == 0) {
                    throw new IllegalArgumentException("RW_ERR_COBS");
                }
                tmp[writeIdx++] = (byte)b;
            }
            if (code < 0xFF && readIdx < end) {
                tmp[writeIdx++] = 0;
            }
        }
        byte[] out = new byte[writeIdx];
        System.arraycopy(tmp, 0, out, 0, writeIdx);
        return out;
    }

    private static void asconSbox32(int[] w) {
        w[0] ^= w[4]; w[4] ^= w[3]; w[2] ^= w[1];
        int t0 = w[0] ^ ((~w[1]) & w[2]);
        int t1 = w[1] ^ ((~w[2]) & w[3]);
        int t2 = w[2] ^ ((~w[3]) & w[4]);
        int t3 = w[3] ^ ((~w[4]) & w[0]);
        int t4 = w[4] ^ ((~w[0]) & w[1]);
        w[1] = t1 ^ t0;
        w[0] = t0 ^ t4;
        w[3] = t3 ^ t2;
        w[2] = ~t2;
        w[4] = t4;
    }

    private static void asconPermute(int[] h, int[] l, int rounds) {
        int start = 12 - rounds;
        for (int r = start; r < 12; r++) {
            l[2] ^= (0xF0 - r * 0x0F);
            asconSbox32(h);
            asconSbox32(l);
            for (int w = 0; w < 5; w++) {
                int hw = h[w], lw = l[w];
                int r0 = ROT0[w], a0 = (r0 >= 32) ? lw : hw, b0 = (r0 >= 32) ? hw : lw, s0 = (r0 >= 32) ? (r0 - 32) : r0;
                int r0h = (a0 >>> s0) | (b0 << (32 - s0));
                int r0l = (b0 >>> s0) | (a0 << (32 - s0));

                int r1 = ROT1[w], a1 = (r1 >= 32) ? lw : hw, b1 = (r1 >= 32) ? hw : lw, s1 = (r1 >= 32) ? (r1 - 32) : r1;
                int r1h = (a1 >>> s1) | (b1 << (32 - s1));
                int r1l = (b1 >>> s1) | (a1 << (32 - s1));

                h[w] = hw ^ r0h ^ r1h;
                l[w] = lw ^ r0l ^ r1l;
            }
        }
    }

    public static byte[] ascon128Sponge(
        byte[] key, byte[] nonce, byte[] ad,
        byte[] buf, int bufOff, int bufLen,
        boolean doCrypt, boolean decrypt
    ) {
        int k0h = readU32BE(key, 0), k0l = readU32BE(key, 4);
        int k1h = readU32BE(key, 8), k1l = readU32BE(key, 12);
        int[] h = { 0x80400C06, k0h, k1h, readU32BE(nonce, 0), readU32BE(nonce, 8) };
        int[] l = { 0x00000000, k0l, k1l, readU32BE(nonce, 4), readU32BE(nonce, 12) };

        asconPermute(h, l, 12);
        h[3] ^= k0h; l[3] ^= k0l;
        h[4] ^= k1h; l[4] ^= k1l;

        byte[] xb = new byte[8];
        if (ad != null && ad.length > 0) {
            int adOff = 0;
            while (adOff + 8 <= ad.length) {
                h[0] ^= readU32BE(ad, adOff);
                l[0] ^= readU32BE(ad, adOff + 4);
                asconPermute(h, l, 6);
                adOff += 8;
            }
            writeU32BE(xb, 0, h[0]); writeU32BE(xb, 4, l[0]);
            for (int j = 0; j < ad.length - adOff; j++) {
                xb[j] ^= ad[adOff + j];
            }
            xb[ad.length - adOff] ^= (byte)0x80;
            h[0] = readU32BE(xb, 0); l[0] = readU32BE(xb, 4);
            asconPermute(h, l, 6);
        }
        l[4] ^= 1;

        int off = 0;
        for (;;) {
            int rem = bufLen - off;
            int n = (rem < 8) ? rem : 8;
            writeU32BE(xb, 0, h[0]); writeU32BE(xb, 4, l[0]);
            for (int j = 0; j < n; j++) {
                int b = buf[bufOff + off + j] & 0xFF;
                if (doCrypt) {
                    if (decrypt) {
                        buf[bufOff + off + j] = (byte)((xb[j] & 0xFF) ^ b);
                        xb[j] = (byte)b;
                    } else {
                        xb[j] = (byte)((xb[j] & 0xFF) ^ b);
                        buf[bufOff + off + j] = xb[j];
                    }
                } else {
                    xb[j] = (byte)((xb[j] & 0xFF) ^ b);
                }
            }
            if (rem >= 8) {
                h[0] = readU32BE(xb, 0); l[0] = readU32BE(xb, 4);
                asconPermute(h, l, 6);
                off += 8;
            } else {
                xb[n] ^= (byte)0x80;
                h[0] = readU32BE(xb, 0); l[0] = readU32BE(xb, 4);
                break;
            }
        }

        h[1] ^= k0h; l[1] ^= k0l;
        h[2] ^= k1h; l[2] ^= k1l;
        asconPermute(h, l, 12);
        h[3] ^= k0h; l[3] ^= k0l;
        h[4] ^= k1h; l[4] ^= k1l;

        byte[] tag = new byte[16];
        writeU32BE(tag, 0, h[3]); writeU32BE(tag, 4, l[3]);
        writeU32BE(tag, 8, h[4]); writeU32BE(tag, 12, l[4]);
        return tag;
    }

    public static byte[] encodeFrame(
        int flags, int channel, int op, int seq,
        byte[] payload, byte[] aeadKey, byte[] aeadNonce
    ) {
        int plen = (payload == null) ? 0 : payload.length;
        boolean hasSec = (flags & (FLAG_AUTH | FLAG_ENC)) != 0;
        int secLen = hasSec ? ASCON_TAG_LEN : 0;
        byte[] raw = new byte[HEADER_LEN + plen + secLen + CRC16_LEN];
        raw[0] = (byte)WIRE_IDENT;
        raw[1] = (byte)flags;
        raw[2] = (byte)channel;
        raw[3] = (byte)op;
        writeU16LE(raw, 4, seq);
        writeU16LE(raw, 6, plen);
        if (plen > 0) {
            System.arraycopy(payload, 0, raw, HEADER_LEN, plen);
        }
        if (hasSec) {
            byte[] ad = new byte[HEADER_LEN];
            System.arraycopy(raw, 0, ad, 0, HEADER_LEN);
            byte[] tag = ascon128Sponge(
                aeadKey, aeadNonce, ad, raw, HEADER_LEN, plen,
                (flags & FLAG_ENC) != 0, false
            );
            System.arraycopy(tag, 0, raw, HEADER_LEN + plen, ASCON_TAG_LEN);
        }
        int crc = crc16Ccitt(raw, 0, raw.length - CRC16_LEN);
        writeU16LE(raw, raw.length - CRC16_LEN, crc);
        byte[] cobs = cobsEncode(raw);
        byte[] wire = new byte[cobs.length + 1];
        System.arraycopy(cobs, 0, wire, 0, cobs.length);
        wire[cobs.length] = (byte)COBS_DELIM;
        return wire;
    }

    public static final int AUDIO_HDR_LEN = 14;
    public static final int AUDIO_SQ_OPEN = 0x01;
    public static final int AUDIO_CLIPPED = 0x02;
    public static final int AUDIO_TX_DIR  = 0x04;
    public static final int AUDIO_SILENCE = 0x08;
    public static final int HELLO_FLAG_DRY_TX = 0x01;

    public static int ulawEncode(int pcm) {
        int sign = 0;
        int mag = pcm;
        if (pcm < 0) { sign = 0x80; mag = -pcm; }
        if (mag > 32635) { mag = 32635; }
        mag += 132;
        int exp = 7;
        int mask = 0x4000;
        while (exp > 0 && (mag & mask) == 0) {
            exp--;
            mask >>>= 1;
        }
        int mant = (mag >>> (exp + 3)) & 0x0F;
        return (~(sign | (exp << 4) | mant)) & 0xFF;
    }

    public static int ulawDecode(int ulaw) {
        int u = (~ulaw) & 0xFF;
        int sign = u & 0x80;
        int exp  = (u >>> 4) & 0x07;
        int mant = u & 0x0F;
        int t = (((mant << 3) + 0x84) << exp) - 0x84;
        return (sign != 0) ? -t : t;
    }

    public static byte[] pack12(int[] samples) {
        int pairs = samples.length >>> 1;
        byte[] out = new byte[pairs * 3];
        int o = 0;
        for (int i = 0; i < pairs * 2; i += 2) {
            int s0 = samples[i] & 0x0FFF;
            int s1 = samples[i + 1] & 0x0FFF;
            out[o++] = (byte)(s0 & 0xFF);
            out[o++] = (byte)(((s0 >>> 8) & 0x0F) | ((s1 & 0x0F) << 4));
            out[o++] = (byte)((s1 >>> 4) & 0xFF);
        }
        return out;
    }

    public static int[] unpack12(byte[] buf) {
        int triples = buf.length / 3;
        int[] out = new int[triples * 2];
        int o = 0;
        for (int i = 0; i < triples * 3; i += 3) {
            int b0 = buf[i] & 0xFF;
            int b1 = buf[i + 1] & 0xFF;
            int b2 = buf[i + 2] & 0xFF;
            out[o++] = b0 | ((b1 & 0x0F) << 8);
            out[o++] = (b1 >>> 4) | (b2 << 4);
        }
        return out;
    }
}
