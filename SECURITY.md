# Security Policy

## Supported Releases

Kern is under active open-source development. Security updates are applied directly to the `master` branch.

| Branch | Supported |
|---|---|
| `master` | :white_check_mark: Active |

## Reporting a Vulnerability

The Kern team takes the security and integrity of embedded firmware, hardware protection, and cryptographic implementations seriously.

If you discover a security issue concerning:
- Memory safety or pointer bounds escapes
- Option byte bypass or RDP Level 1 protection circumvention
- Factory calibration data exposure or corruption
- Cryptographic vulnerabilities in RigWire, Ascon-128, or Ed25519 signature checks
- Out-of-band RF transmission lockout bypasses

**Please do NOT disclose the vulnerability in public issue trackers or forums.**

Instead, please report it privately via GitHub Private Vulnerability Reporting or contact the maintainers directly. Please include:
1. Target hardware revision (UV-K1 / UV-K5 v3)
2. Commit hash or firmware build
3. Detailed proof-of-concept steps or captured trace

We appreciate your cooperation in responsible disclosure to keep the amateur radio community safe.
