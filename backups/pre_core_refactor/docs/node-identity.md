# Protocol-Agnostic Universal Contact Engine Specification (`contacts/`)

In `kern`, a **Contact** defines an *identity* and a *cryptographic relationship*, completely decoupled from RF concepts like frequency or squelch tones (which belong to Channels).

The `contacts` service is a protocol-agnostic, modular core system service (Layer 3) that manages unicast peers and multicast groups in LittleFS (`/sys/contacts.db`). 

---

## 1. Universal Contact Storage Schema (`UniversalContact`)

Contact entries are stored in LittleFS as memory-aligned records. By stripping away RF metadata (like frequencies and subtones), we create a pure cryptographic identity struct:

```rust
#[repr(C, packed)]
pub struct UniversalContact {
    pub contact_id: u32,          // Universal 32-bit primary key ID
    pub contact_type: u8,        // Type: Unicast (0x01), Multicast Group (0x02)
    pub address_type: u8,        // Format: Base40 (0x01), ASCII (0x02), Int32 (0x03), Int16 (0x04)
    pub raw_identity: [u8; 8],   // Raw protocol address bytes (e.g., 6-byte Base40)
    pub display_name: [u8; 12],  // Human-readable UTF-8 / ASCII Alias (e.g. "W2DEF John")
    pub contact_secret: [u8; 32],// 32-Byte Pre-Shared Cryptographic Secret
    pub flags: u16,              // Bitfield: Trusted, Favorite, Repeater, Emergency
    pub reserved: u16,           // Padding for 64-byte alignment
    pub _padding: [u8; 2],       // Pad to exactly 64 bytes
}
```

---

## 2. Passcode-Based Key Derivation (KDF)

Typing 64 hexadecimal characters on a radio keypad to enter a 32-byte `contact_secret` is ergonomically impossible. `kern` adopts the **Hermes Key Derivation Function (KDF)**, allowing users to enter simple text passcodes.

1. **Input**: A human-readable text passcode (e.g., `mesh-alpha-team`) and a 16-byte network salt.
2. **Derivation**: `kern` runs a ChaCha20-based Pseudo-Random Function (PRF) over 10,000 iterations to stretch the passcode.
3. **Output**: A cryptographically secure 256-bit (32-byte) `contact_secret` stored in the `UniversalContact` record.

### 2.1. Versatile Applications of the 32-Byte Secret

The derived 32-byte `contact_secret` adapts dynamically to whatever protocol or service is active:

- **Hermes Scope Secret ($K_{scope}$)**: Pairwise unicast or group multicast key for ChaCha20-Poly1305 AEAD packet encryption.
- **DSP-Sec Voice Scrambler Seed**: Pseudorandom seed for generating dynamic frequency-inversion hopping patterns for analog FM.
- **Rigwire Auth Token**: Key for Poly1305 MAC tag generation.

---

## 3. Reserved Protocol Addresses

Rather than polluting the LittleFS database with hardcoded discovery or broadcast contacts, `kern` delegates reserved routing addresses to the protocol parsers. 

For the Hermes protocol, the following 48-bit Base40 addresses are natively intercepted by the firmware without requiring a contact entry:

| 48-Bit Base40 Address | Concept | Routing Behavior |
|---|---|---|
| `0xFFFFFFFFFFFF` | **Broadcast (`ALLCALL`)** | Delivered to every node listening. Unencrypted or signed with the public Network Key ($K_{net}$). |
| `0xDDDDDDDDDDDD` | **Discovery** | Open beacon channel. Nodes transmit their capabilities and public keys here to populate neighbor caches. |

---

## 4. Universal Resolver Interface (`ContactResolver`)

Services and apps query the contact engine via a single unified trait:

```rust
pub trait ContactResolver {
    /// Resolves incoming identity bytes & address type to a UniversalContact.
    fn resolve(&self, address_type: u8, raw_id: &[u8]) -> Option<UniversalContact>;

    /// Fetches the 32-byte secret for a given contact ID.
    fn get_secret(&self, contact_id: u32) -> Option<[u8; 32]>;

    /// Adds or updates a contact entry in LittleFS, returning the internal ID.
    fn save_contact(&mut self, contact: &UniversalContact) -> Result<u32, FsError>;
}
```

### 4.1. Automatic Resolution Pipeline

### 2.2. The Active Link Scenario

**Incoming Voice / Packet (Hermes / CTCSS / Rigwire)**
1. A node tag arrives via UART (Rigwire), CTCSS squelch decoder, or an FSK burst (Hermes).
2. Resolver queries 192-byte SRAM LRU Cache
                           │
                           ▼
              Found Contact: "W2DEF John"
                           │
            Extract 32-Byte `contact_secret`
                           │
      ┌────────────────────┴────────────────────┐
      ▼                                         ▼
Hermes AEAD Decrypt                      DSP-Sec Descramble
(ChaCha20-Poly1305 Payload)              (Apply Carrier Hopping)
```
