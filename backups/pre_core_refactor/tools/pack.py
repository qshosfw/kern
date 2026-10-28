import sys

OBFUS = bytes([0x16, 0x6c, 0x14, 0xe6, 0x2e, 0x91, 0x0d, 0x40,
               0x21, 0x35, 0xd5, 0x40, 0x13, 0x03, 0xe9, 0x80])

def pack(fin, fout):
    with open(fin, 'rb') as f:
        data = bytearray(f.read())
    
    # Pad to 16 bytes
    while len(data) % 16 != 0:
        data.append(0xFF)
        
    for i in range(len(data)):
        data[i] ^= OBFUS[i % 16]
        
    with open(fout, 'wb') as f:
        f.write(data)

pack(sys.argv[1], sys.argv[2])
