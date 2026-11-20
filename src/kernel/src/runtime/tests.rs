use super::*;
use ::abi::CapabilitySet;

#[test]
fn test_krx_swap_header_serde() {
    let header = KrxSwapHeader::new(2, 64, 28, SWAP_FLAG_COMPRESSED_RLE, 0xABCD);
    assert!(header.is_valid());
    assert!(header.is_rle());
    assert_eq!(core::mem::size_of::<KrxSwapHeader>(), 16);

    let bytes = header.to_bytes();
    assert_eq!(bytes.len(), 16);

    let parsed = KrxSwapHeader::parse(&bytes).expect("Failed to parse swap header");
    let ident = parsed.ident;
    let app_id = parsed.app_id;
    let orig_len = parsed.original_len;
    let comp_len = parsed.compressed_len;
    let flags = parsed.flags;
    let crc = parsed.crc16;

    assert_eq!(ident, SWAP_IDENT);
    assert_eq!(app_id, 2);
    assert_eq!(orig_len, 64);
    assert_eq!(comp_len, 28);
    assert_eq!(flags, SWAP_FLAG_COMPRESSED_RLE);
    assert_eq!(crc, 0xABCD);
}

#[test]
fn test_service_export_serde() {
    assert_eq!(core::mem::size_of::<ServiceExport>(), 16);
    let mut raw = [0u8; 16];
    raw[0..2].copy_from_slice(&0x0104u16.to_le_bytes()); // interface_id
    raw[2..4].copy_from_slice(&2u16.to_le_bytes()); // abi_version
    raw[4..8].copy_from_slice(&0x0000_0140u32.to_le_bytes()); // vtable_offset
    raw[8..10].copy_from_slice(&ServiceExport::FLAG_PINNED.to_le_bytes()); // flags

    let export = ServiceExport::parse(&raw).expect("Failed to parse ServiceExport");
    let iface = export.interface_id;
    let abi = export.abi_version;
    let vtable_off = export.vtable_offset;
    assert_eq!(iface, 0x0104);
    assert_eq!(abi, 2);
    assert_eq!(vtable_off, 0x0140);
    assert!(export.is_pinned());
}

#[test]
fn test_section_execution_can_execute() {
    let mut raw = [0u8; 28];
    // min_stack = 256
    raw[0..2].copy_from_slice(&256u16.to_le_bytes());
    // min_heap = 512
    raw[2..4].copy_from_slice(&512u16.to_le_bytes());

    let sec = SectionExecution::parse(&raw).expect("Failed to parse SectionExecution");
    // Total required = 256 + 512 + 128 = 896
    assert!(sec.can_execute(1024, 128));
    assert!(sec.can_execute(896, 128));
    assert!(!sec.can_execute(895, 128));
    assert!(!sec.can_execute(512, 128));
}

#[test]
fn test_packbits_compression_roundtrip() {
    let original = [
        1, 2, 3, 4, 5, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 10, 20, 30, 0xBB, 0xBB, 0xBB, 99,
    ];
    let mut compressed = [0u8; 64];
    let comp_len = packbits_compress(&original, &mut compressed).expect("Compression failed");

    let mut restored = [0u8; 64];
    let decomp_len =
        packbits_decompress(&compressed[..comp_len], &mut restored).expect("Decompression failed");

    assert_eq!(decomp_len, original.len());
    assert_eq!(&restored[..decomp_len], &original[..]);
}

#[test]
fn test_packbits_repeated_runs() {
    let original = [0u8; 100];
    // 100 zeros
    let mut compressed = [0u8; 64];
    let comp_len = packbits_compress(&original, &mut compressed).expect("Compression failed");
    // A single run of 100 identical bytes should take exactly 2 bytes!
    assert_eq!(comp_len, 2);

    let mut restored = [0xFFu8; 100];
    let decomp_len =
        packbits_decompress(&compressed[..comp_len], &mut restored).expect("Decompression failed");
    assert_eq!(decomp_len, 100);
    assert!(restored.iter().all(|&b| b == 0));
}

#[test]
fn test_packbits_pairs_and_long_literals_roundtrip() {
    let mut original = [0u8; 300];
    for (i, b) in original.iter_mut().enumerate() {
        *b = (i * 7) as u8;
    }
    original[10] = 0x55;
    original[11] = 0x55;
    original[298] = 0x42;
    original[299] = 0x42;
    let mut compressed = [0u8; 320];
    let comp_len = packbits_compress(&original, &mut compressed).expect("Compression failed");
    let mut restored = [0u8; 300];
    let decomp_len =
        packbits_decompress(&compressed[..comp_len], &mut restored).expect("Decompression failed");
    assert_eq!(&restored[..decomp_len], &original[..]);
    assert!(packbits_compress(&original, &mut compressed[..8]).is_err());
}

#[test]
fn test_krx_launch_error_diagnostics() {
    use ::abi::Cap;

    let err_cap = KrxLaunchError::CapabilityDenied(crate::caps::name(Cap::BroadBandRx));
    assert_eq!(err_cap.title(), "Missing Hardware");
    assert_eq!(err_cap.message(), "Broadband HF RX");

    let err_ram = KrxLaunchError::InsufficientRam;
    assert_eq!(err_ram.title(), "Out of RAM");
    assert_eq!(err_ram.message(), "Not enough RAM");

    let err_stack = KrxLaunchError::InsufficientStack;
    assert_eq!(err_stack.title(), "Stack Overflow");
    assert_eq!(err_stack.message(), "Stack headroom low");

    let err_fnf = KrxLaunchError::FileNotFound;
    assert_eq!(err_fnf.title(), "File Not Found");
    assert_eq!(err_fnf.message(), "File not found");

    let err_reloc = KrxLaunchError::RelocationFailed;
    assert_eq!(err_reloc.title(), "Reloc Error");
    assert_eq!(err_reloc.message(), "Relocation failed");

    let err_api = KrxLaunchError::ImportBindingFailed;
    assert_eq!(err_api.title(), "API Unresolved");
    assert_eq!(err_api.message(), "API not available");

    let err_corrupt = KrxLaunchError::MissingExecution;
    assert_eq!(err_corrupt.title(), "Corrupted Binary");
    assert_eq!(err_corrupt.message(), "Missing manifest");
}

#[test]
fn test_leaf_continuation_and_context() {
    assert_eq!(core::mem::size_of::<LeafContext>(), 16);
    assert_eq!(core::mem::size_of::<LeafContinuation>(), 8);

    let cont = LeafContinuation::call_leaf(5, 0x1234_5678);
    assert!(cont.is_call_leaf());
    let raw = cont.as_u64();
    let restored = LeafContinuation::from_u64(raw);
    assert_eq!(cont, restored);
    assert_eq!(restored.target_or_code, 5);
    assert_eq!(restored.payload, 0x1234_5678);

    let ctx = LeafContext::new(3, 42);
    assert_eq!(ctx.leaf_id, 3);
    let arg = ctx.arg;
    assert_eq!(arg, 42);
    let cont = ctx.continuation;
    assert!(cont.is_done());
}

#[test]
fn test_krx_profile_detection_and_inference() {
    let mut preamble_bytes = [0u8; 24];
    preamble_bytes[0..4].copy_from_slice(&KRX_IDENT);
    preamble_bytes[4] = KrxClass::Native as u8;
    preamble_bytes[5] = 2; // format_version
    preamble_bytes[6..8].copy_from_slice(&1u16.to_le_bytes()); // section_count
    let preamble = KrxPreamble::parse(&preamble_bytes).unwrap();

    let mut exec = SectionExecution {
        entry_point: 0x10,
        got_offset: 0x40,
        reloc_offset: 0,
        reloc_count: 0,
        min_stack_bytes: 256,
        min_heap_bytes: 256,
        page_size: 0,
        cache_blocks: 0,
        dma_burst_size: 0,
        payload_size: 1024,
        rt_pin_mask: 0,
        max_fault_latency_us: 0,
        reserved: [0u8; 2],
    };

    // Class Native + reloc_count == 0 + link_base == 0 => N1NativePic
    assert_eq!(
        KrxProfile::detect(&preamble, &exec, false, false),
        KrxProfile::N1NativePic
    );

    // Class Native + reloc_count == 0 + link_base != 0 => N0NativeFixed
    let mut fixed_preamble_bytes = preamble_bytes;
    fixed_preamble_bytes[20..24].copy_from_slice(&0x2000_1000u32.to_le_bytes());
    let fixed_preamble = KrxPreamble::parse(&fixed_preamble_bytes).unwrap();
    assert_eq!(
        KrxProfile::detect(&fixed_preamble, &exec, false, false),
        KrxProfile::N0NativeFixed
    );

    // Explicit profile stamped into reserved[0] overrides detection
    exec.reserved[0] = KrxProfile::P0StructuredPaged.raw_code();
    assert_eq!(
        KrxProfile::detect(&preamble, &exec, false, false),
        KrxProfile::P0StructuredPaged
    );

    exec.reserved[0] = 0;
    // Class Paged + has_leaf_map => P0StructuredPaged
    let mut paged_bytes = preamble_bytes;
    paged_bytes[4] = KrxClass::Paged as u8;
    let paged_preamble = KrxPreamble::parse(&paged_bytes).unwrap();
    assert_eq!(
        KrxProfile::detect(&paged_preamble, &exec, false, true),
        KrxProfile::P0StructuredPaged
    );
    // Class Paged + no leaf map => P1TransparentPaged
    assert_eq!(
        KrxProfile::detect(&paged_preamble, &exec, true, false),
        KrxProfile::P1TransparentPaged
    );
}

#[test]
fn test_synthetic_krx_package_end_to_end_parsing() {
    let mut pkg = [0u8; 180];

    // 1. Preamble (24 bytes)
    pkg[0..4].copy_from_slice(&KRX_IDENT);
    pkg[4] = KrxClass::Paged as u8;
    pkg[5] = 5; // format version 2.5.0
    pkg[6..8].copy_from_slice(&4u16.to_le_bytes()); // 4 sections
    let header_len = 148u16;
    pkg[8..10].copy_from_slice(&header_len.to_le_bytes());

    // 2. SectionIdentity (offset 24: 4B header + 40B body = 44B, ends at 68)
    pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
    pkg[26..28].copy_from_slice(&44u16.to_le_bytes());
    let name_str_off = 132u16;
    pkg[28..30].copy_from_slice(&name_str_off.to_le_bytes());
    pkg[30..33].copy_from_slice(&[1, 2, 0]); // v1.2.0
    pkg[33..35].copy_from_slice(&1u16.to_le_bytes()); // min_abi = 1
    pkg[35..41].copy_from_slice(b"F4HWN\0");

    // 3. SectionExecution (offset 68: 4B header + 28B body = 32B, ends at 100)
    pkg[68..70].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
    pkg[70..72].copy_from_slice(&32u16.to_le_bytes());
    pkg[72..74].copy_from_slice(&256u16.to_le_bytes()); // min_stack = 256
    pkg[76..78].copy_from_slice(&1024u16.to_le_bytes()); // page_size = 1024
    pkg[78] = 2; // cache_blocks = 2
    pkg[80..84].copy_from_slice(&0x08u32.to_le_bytes()); // entry_point = 0x08
    pkg[84..88].copy_from_slice(&32u32.to_le_bytes()); // payload_size = 32

    // 4. SectionCapImports (offset 100: 4B header + 8B body = 12B, ends at 112)
    pkg[100..102].copy_from_slice(&SECTION_CAP_IMPORTS.to_le_bytes());
    pkg[102..104].copy_from_slice(&12u16.to_le_bytes());
    pkg[104..106].copy_from_slice(&0x0001u16.to_le_bytes()); // interface 1 (Display)
    pkg[106..108].copy_from_slice(&1u16.to_le_bytes()); // min_abi = 1
    pkg[108..110].copy_from_slice(&1u16.to_le_bytes()); // flags = mandatory
    pkg[110..112].copy_from_slice(&0u16.to_le_bytes()); // slot = 0

    // 5. SectionLeafMap (offset 112: 4B header + 16B body = 20B, ends at 132)
    pkg[112..114].copy_from_slice(&SECTION_LEAF_MAP.to_le_bytes());
    pkg[114..116].copy_from_slice(&20u16.to_le_bytes());
    pkg[116] = 1; // leaf_id = 1
    pkg[117] = 0; // flags = 0
    pkg[118..120].copy_from_slice(&512u16.to_le_bytes()); // size = 512
    pkg[120..124].copy_from_slice(&148u32.to_le_bytes()); // flash_offset = 148
    pkg[124..126].copy_from_slice(&0u16.to_le_bytes()); // entry_offset = 0
    pkg[126..128].copy_from_slice(&0u16.to_le_bytes()); // ram_scratch = 0

    // 6. String Pool (offset 132..148)
    pkg[132..138].copy_from_slice(b"Beacon");
    let name_str_off = 132u16;
    pkg[28..30].copy_from_slice(&name_str_off.to_le_bytes());

    let header_len = 148u16;
    pkg[8..10].copy_from_slice(&header_len.to_le_bytes());

    // Validate parsing
    let preamble = KrxPreamble::parse(&pkg).unwrap();
    let section_count = preamble.section_count;
    let hlen = preamble.header_len;
    assert_eq!(section_count, 4);
    assert_eq!(hlen, 148);

    // Find Identity
    let ident_body = find_section(&pkg, 148, SECTION_IDENTITY).unwrap();
    let ident = SectionIdentity::parse(ident_body).unwrap();
    assert_eq!(ident.name(&pkg[..148]).unwrap(), "Beacon");

    // Find Execution
    let exec_body = find_section(&pkg, 148, SECTION_EXECUTION).unwrap();
    let exec = SectionExecution::parse(exec_body).unwrap();
    let ep = exec.entry_point;
    let ps = exec.payload_size;
    assert_eq!(ep, 0x08);
    assert_eq!(ps, 32);

    // Find CapImports
    let cap_body = find_section(&pkg, 148, SECTION_CAP_IMPORTS).unwrap();
    let binding = CapBinding::parse(cap_body, 0).unwrap();
    let iface = binding.interface_id;
    let slot = binding.slot_idx;
    assert_eq!(iface, 0x0001);
    assert_eq!(slot, 0);

    // Find LeafMap
    let leaf_body = find_section(&pkg, 148, SECTION_LEAF_MAP).unwrap();
    let leaf = LeafEntry::parse(leaf_body).unwrap();
    let lsize = leaf.size;
    assert_eq!(leaf.leaf_id, 1);
    assert_eq!(lsize, 512);

    // Detect profile: Paged class + leaf map => P0StructuredPaged
    let profile = KrxProfile::detect(&preamble, &exec, false, true);
    assert_eq!(profile, KrxProfile::P0StructuredPaged);
}

#[test]
fn test_native_provisioned_package_full_test_bench() {
    use crate::proc::ProcessTable;
    use crate::proc::abi::*;
    use crate::proc::memory::MemoryMapLayout;
    use ::abi::Cap;

    // ── 1. Synthesize Native Provisioned Package Binary (/apps/beacon.krx) ──
    let mut pkg = [0u8; 148]; // 132B header + 16B code payload

    // 1a. Preamble (24B)
    pkg[0..4].copy_from_slice(&KRX_IDENT);
    pkg[4] = KrxClass::Native as u8;
    pkg[5] = 0x05; // v2.5.0
    pkg[6..8].copy_from_slice(&4u16.to_le_bytes()); // 4 sections
    let header_len = 132u16;
    pkg[8..10].copy_from_slice(&header_len.to_le_bytes());

    // 1b. SectionIdentity (offset 24: 4B header + 40B body = 44B, ends at 68)
    pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
    pkg[26] = 44;
    pkg[28] = 116; // name_str offset = 116
    pkg[30] = 1; // version 1.0.0
    pkg[33] = 1; // min_kernel_abi = 1
    pkg[35..44].copy_from_slice(b"KERN-TEAM");
    pkg[51] = 1; // icon_asset = 1

    // 1c. SectionExecution (offset 68: 4B header + 28B body = 32B, ends at 100)
    pkg[68..70].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
    pkg[70] = 32;
    pkg[72..74].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes = 256
    pkg[74..76].copy_from_slice(&128u16.to_le_bytes()); // min_heap_bytes = 128
    pkg[76..78].copy_from_slice(&0u16.to_le_bytes()); // page_size = 0 (unpaged native)
    pkg[78] = 0; // cache_blocks = 0
    pkg[80..84].copy_from_slice(&0u32.to_le_bytes()); // entry_point = 0 (offset in payload)
    pkg[84..88].copy_from_slice(&16u32.to_le_bytes()); // payload_size = 16B
    pkg[92..94].copy_from_slice(&16u16.to_le_bytes()); // got_offset = 16B
    pkg[98] = KrxProfile::N1NativePic.raw_code(); // explicit profile

    // 1d. SectionType (offset 100: 4B header + 12B body = 16B, ends at 116)
    pkg[100..102].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
    pkg[102] = 16;
    pkg[104] = KrxExecType::App as u8;
    pkg[106] = 1; // stack_cost = 1
    let required_caps = (Cap::DisplayDirect as u32) | (Cap::Keypad as u32) | (Cap::RadioRx as u32);
    pkg[112..116].copy_from_slice(&required_caps.to_le_bytes());

    // 1e. String pool (offset 116..132)
    pkg[116..122].copy_from_slice(b"Beacon");

    // 1f. Thumb-1 Native Code Payload (offset 132..148, 16 bytes)
    let thumb1_code: [u8; 16] = [
        0x02, 0x28, // cmp r0, #2 (KRX_OP_EVENT)
        0x01, 0xd1, // bne +1
        0x01, 0x20, // movs r0, #1 (Consumed)
        0x70, 0x47, // bx lr
        0x00, 0x20, // movs r0, #0 (Success / Ignored)
        0x70, 0x47, // bx lr
        0x00, 0x00, 0x00, 0x00, // alignment padding
    ];
    pkg[132..148].copy_from_slice(&thumb1_code);

    // ── 2. Validate End-to-End Preamble and Section Scanning ──
    let preamble = KrxPreamble::parse(&pkg).expect("Valid KRX preamble");
    assert_eq!(preamble.ident, KRX_IDENT);
    assert_eq!(preamble.execution_class(), Some(KrxClass::Native));
    assert_eq!(preamble.format_version, 5);
    let hlen = preamble.header_len;
    assert_eq!(hlen, 132);

    let mut scanned_ident = false;
    let mut scanned_exec = false;
    let mut scanned_type = false;

    for (hdr, body) in KrxSectionIterator::new(&pkg, 132) {
        match hdr.id {
            SECTION_IDENTITY => {
                let ident = SectionIdentity::parse(body).expect("Valid Identity");
                assert_eq!(ident.name(&pkg[..132]).unwrap(), "Beacon");
                assert_eq!(ident.version, [1, 0, 0]);
                scanned_ident = true;
            }
            SECTION_EXECUTION => {
                let exec = SectionExecution::parse(body).expect("Valid Execution");
                let min_stack = exec.min_stack_bytes;
                let payload_sz = exec.payload_size;
                let got_off = exec.got_offset;
                assert_eq!(min_stack, 256);
                assert_eq!(payload_sz, 16);
                assert_eq!(got_off, 16);
                assert_eq!(exec.reserved[0], KrxProfile::N1NativePic.raw_code());
                // Verify RAM / stack margin headroom evaluation
                assert!(exec.can_execute(1024, 256));
                assert!(!exec.can_execute(300, 256));
                scanned_exec = true;
            }
            SECTION_TYPE_LIFECYCLE => {
                let st = SectionType::parse(body).expect("Valid Type");
                assert_eq!(st.exec_type, KrxExecType::App as u8);
                let cap_set = st.capabilities();
                assert!(cap_set.contains(Cap::DisplayDirect));
                assert!(cap_set.contains(Cap::Keypad));
                assert!(cap_set.contains(Cap::RadioRx));
                assert!(!cap_set.contains(Cap::BroadBandRx));
                scanned_type = true;
            }
            _ => {}
        }
    }
    assert!(scanned_ident && scanned_exec && scanned_type);

    // ── 3. Validate Unified Single-Entry ABI Dispatch ──
    let stub_api = KrxApiTable::safe_stub(required_caps);
    unsafe {
        assert_eq!(stub_api.invoke(::abi::Op::RfGet, 0, 0, 0), 0);
        assert_eq!(
            stub_api.invoke(::abi::Op::RfTune, 145_000_000, 1, 12_500),
            1
        );
        assert_eq!(stub_api.invoke(::abi::Op::GfxFb, 0, 0, 0), 0);
    }

    // ── 4. Validate Process Lifecycle & PCB Ledger Integration ──
    let mut proc_table = ProcessTable::<4>::new();
    let pid = proc_table
        .spawn(
            "Beacon",
            1,
            crate::sched::Priority::Ui,
            crate::proc::ExecType::RelocatableNative,
            CapabilitySet::from_raw(required_caps),
        )
        .expect("Spawn Beacon process");
    assert_eq!(pid, crate::proc::Pid(1));

    let pcb = proc_table.find_mut(pid).expect("Found PCB");
    assert_eq!(pcb.state, crate::proc::ProcState::Ready);
    pcb.set_state(crate::proc::ProcState::Running);
    pcb.ledger.record_activation();
    pcb.ledger.record_event();
    assert_eq!(pcb.ledger.cpu_ticks, 1);
    assert_eq!(pcb.ledger.ipc_events, 1);

    // ── 5. Validate MemoryMapLayout Boundary & Access Protection ──
    let mut mem_layout = MemoryMapLayout::<4>::new();
    let load_base = 0x2000_1000u32;
    let code_size = 16u32;
    let ram_vma = load_base + 16;
    let ram_size = 256u32;
    mem_layout.init_native_pic(load_base, code_size, ram_vma, ram_size);

    // Code region: Execute = OK, Write = Denied
    assert!(mem_layout.validate_access(load_base, 2, false, true));
    assert!(mem_layout.validate_access(load_base + 8, 4, false, false));
    assert!(!mem_layout.validate_access(load_base, 4, true, false));

    // RAM/GOT region: Write = OK, Execute = Denied
    assert!(mem_layout.validate_access(ram_vma, 4, true, false));
    assert!(mem_layout.validate_access(ram_vma + 32, 4, false, false));
    assert!(!mem_layout.validate_access(ram_vma, 2, false, true));

    // Out of bounds: Denied
    assert!(!mem_layout.validate_access(load_base - 4, 4, false, false));
    assert!(!mem_layout.validate_access(ram_vma + ram_size + 4, 4, false, false));

    // ── 6. Full C-ABI Safe-Stub Execution Dispatch Test ──
    let api_table = KrxApiTable::safe_stub(required_caps);
    assert_eq!(api_table.abi_version, KRX_ABI_VERSION);
    assert_eq!(api_table.granted_caps, required_caps);

    // Test Simulated Guest Entry Point Handler:
    // Demonstrates standard lifecycle routing and safe stub calls
    let guest_handler = |op: u32, api: *const KrxApiTable, param: *const ()| -> u32 {
        let table = unsafe { &*api };
        match op {
            KRX_OP_START => {
                // Initialize app state
                0
            }
            KRX_OP_EVENT => {
                if param.is_null() {
                    return 0;
                }
                let ev = unsafe { &*(param as *const KrxEvent) };
                if ev.is_key() {
                    1 // Consumed
                } else if ev.is_radio_bus() {
                    let _freq = unsafe { table.invoke(::abi::Op::RfGet, 0, 0, 0) };
                    1 // Consumed
                } else {
                    0 // Ignored
                }
            }
            KRX_OP_RENDER => {
                unsafe {
                    table.invoke(::abi::Op::GfxDraw, 1 | (1 << 8), 0, (128) | (64 << 16));
                    table.invoke(::abi::Op::GfxCommit, 0, 0, 0);
                }
                0
            }
            KRX_OP_EXIT => 0,
            _ => 0xFFFF_FFFF,
        }
    };

    // Dispatch KRX_OP_START
    assert_eq!(
        guest_handler(KRX_OP_START, &api_table, core::ptr::null()),
        0
    );

    // Dispatch KRX_OP_EVENT (Key Press)
    let key_ev = KrxEvent {
        kind: KRX_EV_KEY,
        key: 15,
        flags: 0,
        reserved: 0,
        payload: 0, // KeyState::Pressed
    };
    assert_eq!(
        guest_handler(KRX_OP_EVENT, &api_table, &key_ev as *const _ as *const ()),
        1
    );

    // Dispatch KRX_OP_EVENT (Radio Bus)
    let bus_ev = KrxEvent {
        kind: KRX_EV_BUS,
        key: 1,
        flags: (crate::ipc::Topic::RADIO.0 & 0xFF) as u8,
        reserved: 0,
        payload: 145_000_000,
    };
    assert_eq!(
        guest_handler(KRX_OP_EVENT, &api_table, &bus_ev as *const _ as *const ()),
        1
    );

    // Dispatch KRX_OP_RENDER
    assert_eq!(
        guest_handler(KRX_OP_RENDER, &api_table, core::ptr::null()),
        0
    );

    // Dispatch KRX_OP_EXIT
    assert_eq!(guest_handler(KRX_OP_EXIT, &api_table, core::ptr::null()), 0);

    // Terminate process
    let pcb = proc_table.find_mut(pid).expect("Found PCB");
    pcb.set_state(crate::proc::ProcState::Dead);
    assert!(!pcb.is_active());
}

#[test]
fn test_native_execution_fault_matrix_test_bench() {
    use ::abi::{Cap, CapabilitySet};

    // 1. Fault: Corrupted Preamble FourCC
    let mut bad_ident = [0u8; 32];
    bad_ident[0..4].copy_from_slice(b"BAD\0");
    assert!(KrxPreamble::parse(&bad_ident).is_err());

    // 2. Fault: Unsupported Format Version
    let mut bad_ver = [0u8; 32];
    bad_ver[0..4].copy_from_slice(&KRX_IDENT);
    bad_ver[5] = 99; // Version 99.0
    assert!(KrxPreamble::parse(&bad_ver).is_err());

    // 3. Fault: Truncated Package Header
    let mut truncated = [0u8; 64];
    truncated[0..4].copy_from_slice(&KRX_IDENT);
    truncated[4] = KrxClass::Native as u8;
    truncated[5] = 5;
    truncated[8..10].copy_from_slice(&128u16.to_le_bytes()); // header_len = 128, but only 64B present!
    let preamble = KrxPreamble::parse(&truncated).unwrap();
    assert!(64 < preamble.header_len as usize); // Truncation detected!

    // 4. Fault: Missing Identity Section
    let mut no_ident = [0u8; 128];
    no_ident[0..4].copy_from_slice(&KRX_IDENT);
    no_ident[4] = KrxClass::Native as u8;
    no_ident[5] = 5;
    no_ident[6..8].copy_from_slice(&1u16.to_le_bytes());
    no_ident[8..10].copy_from_slice(&64u16.to_le_bytes());
    no_ident[24..26].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
    no_ident[26] = 32;
    let mut found_ident = false;
    for (hdr, _) in KrxSectionIterator::new(&no_ident, 64) {
        if hdr.id == SECTION_IDENTITY {
            found_ident = true;
        }
    }
    assert!(!found_ident); // Correctly missing!

    // 5. Fault: Capability Denied (requires BroadBandRx, platform only has Display + Keypad)
    let platform_caps = CapabilitySet::from_raw((Cap::DisplayDirect as u32) | (Cap::Keypad as u32));
    let app_required_caps =
        CapabilitySet::from_raw((Cap::DisplayDirect as u32) | (Cap::BroadBandRx as u32));
    assert!(!platform_caps.contains_all(app_required_caps));
    let missing = app_required_caps.without(platform_caps);
    assert_eq!(missing.first_cap(), Some(Cap::BroadBandRx));
    let err = KrxLaunchError::CapabilityDenied(crate::caps::name(missing.first_cap().unwrap()));
    assert_eq!(err.message(), "Broadband HF RX");

    // 6. Fault: Insufficient Stack Headroom
    let mut exec_raw = [0u8; 28];
    exec_raw[0..2].copy_from_slice(&512u16.to_le_bytes()); // min_stack_bytes = 512
    let sec_exec = SectionExecution::parse(&exec_raw).unwrap();
    let current_stack_margin = 256usize;
    assert!(current_stack_margin < sec_exec.min_stack_bytes as usize);

    // 7. Fault: Insufficient System RAM
    exec_raw[2..4].copy_from_slice(&4096u16.to_le_bytes()); // min_heap_bytes = 4096
    let sec_exec_ram = SectionExecution::parse(&exec_raw).unwrap();
    let available_ram = 2048usize;
    assert!(!sec_exec_ram.can_execute(available_ram, 256));

    // 8. Fault: Relocation Truncation / Stream Corrupt
    let mut reloc_buffer = [0u32; 16];
    let reloc_stream = [0xFFu8, 0xFFu8]; // Truncated escape stream
    let res = unsafe {
        super::reloc::apply_relocs(
            reloc_buffer.as_mut_ptr() as *mut u8,
            64,
            &reloc_stream,
            0x1000,
            5, // Expects 5 relocations, only 2 bytes provided
        )
    };
    assert!(res.is_err());
}

#[test]
fn test_native_app_execution_and_abi_integration_bench() {
    use crate::proc::abi::*;
    use ::abi::Cap;

    // Construct a complete Native Package in memory:
    let mut pkg = [0u8; 320];
    let required_caps = (Cap::DisplayDirect as u32) | (Cap::Keypad as u32) | (Cap::RadioRx as u32);

    // 1. Preamble (24 bytes)
    pkg[0..4].copy_from_slice(&KRX_IDENT);
    pkg[4] = KrxClass::Native as u8;
    pkg[5] = 5; // format version 2.5.0
    pkg[6..8].copy_from_slice(&3u16.to_le_bytes()); // 3 sections
    pkg[8..10].copy_from_slice(&132u16.to_le_bytes()); // header_len = 132

    // 2. SectionIdentity (offset 24: 4B header + 40B body)
    pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
    pkg[26] = 44;
    pkg[28] = 116; // name_str offset = 116
    pkg[30] = 1; // v1.0.0
    pkg[33] = 1; // min_abi = 1
    pkg[35..44].copy_from_slice(b"KERN-TEAM");

    // 3. SectionExecution (offset 68: 4B header + 28B body)
    pkg[68..70].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
    pkg[70] = 32;
    pkg[72..74].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes = 256
    pkg[76..78].copy_from_slice(&512u16.to_le_bytes()); // page_size = 512
    pkg[84..88].copy_from_slice(&176u32.to_le_bytes()); // payload_size = 176
    pkg[92..94].copy_from_slice(&176u16.to_le_bytes()); // got_offset
    pkg[98] = KrxProfile::N1NativePic.raw_code();

    // 4. SectionType (offset 100: 4B header + 12B body)
    pkg[100..102].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
    pkg[102] = 16;
    pkg[104] = KrxExecType::App as u8;
    pkg[112..116].copy_from_slice(&required_caps.to_le_bytes());

    // 5. String pool (offset 116..132)
    pkg[116..122].copy_from_slice(b"Beacon");

    // Verify preamble and sections parse correctly
    let preamble = KrxPreamble::parse(&pkg).expect("Valid preamble");
    assert_eq!(preamble.execution_class(), Some(KrxClass::Native));

    let mut ident_found = false;
    let mut exec_found = false;
    let mut type_found = false;
    for (hdr, body) in KrxSectionIterator::new(&pkg, 132) {
        match hdr.id {
            SECTION_IDENTITY => {
                let ident = SectionIdentity::parse(body).unwrap();
                assert_eq!(ident.name(&pkg[..132]), Some("Beacon"));
                ident_found = true;
            }
            SECTION_EXECUTION => {
                let exec = SectionExecution::parse(body).unwrap();
                let payload_sz = exec.payload_size;
                assert_eq!(payload_sz, 176);
                assert_eq!(exec.profile(), Some(KrxProfile::N1NativePic));
                exec_found = true;
            }
            SECTION_TYPE_LIFECYCLE => {
                let st = SectionType::parse(body).unwrap();
                let cap_mask = st.cap_mask;
                assert_eq!(cap_mask, required_caps);
                type_found = true;
            }
            _ => {}
        }
    }
    assert!(ident_found && exec_found && type_found);

    // Test Simulated Full App Lifecycle with state and ABI calls
    struct TestAppState {
        freq: u32,
        rendered_frames: u32,
        key_presses: u32,
        exited: bool,
    }
    static mut APP_STATE: TestAppState = TestAppState {
        freq: 0,
        rendered_frames: 0,
        key_presses: 0,
        exited: false,
    };

    let app_entry = |op: u32, api: *const KrxApiTable, param: *const ()| -> u32 {
        let table = unsafe { &*api };
        let state = unsafe { &mut *core::ptr::addr_of_mut!(APP_STATE) };
        match op {
            KRX_OP_START => {
                state.freq = 145_500_000;
                unsafe { table.invoke(::abi::Op::RfTune, state.freq as usize, 1, 12_500) };
                0
            }
            KRX_OP_EVENT => {
                if param.is_null() {
                    return 0;
                }
                let ev = unsafe { &*(param as *const KrxEvent) };
                if ev.is_key() {
                    state.key_presses += 1;
                    if ev.key == 15 {
                        0 // KEY_EXIT: pass to system
                    } else {
                        1 // Consumed
                    }
                } else {
                    0
                }
            }
            KRX_OP_RENDER => {
                state.rendered_frames += 1;
                let req = KrxTextReq {
                    text: b"BEACON".as_ptr(),
                    x: 16,
                    y: 26,
                    len: 6,
                    arg: 0,
                    font: 0,
                    color: 1,
                    flags: 0,
                    _pad: 0,
                };
                unsafe {
                    table.invoke(::abi::Op::GfxDraw, 1 | (1 << 8), 0, 128 | (64 << 16));
                    table.invoke(::abi::Op::GfxText, &raw const req as usize, 0, 0);
                    table.invoke(::abi::Op::GfxCommit, 0, 0, 0);
                }
                0
            }
            KRX_OP_EXIT => {
                state.exited = true;
                0
            }
            _ => 0xFFFF_FFFF,
        }
    };

    let api_table = KrxApiTable::safe_stub(required_caps);

    // 1. START
    assert_eq!(app_entry(KRX_OP_START, &api_table, core::ptr::null()), 0);
    assert_eq!(unsafe { APP_STATE.freq }, 145_500_000);

    // 2. RENDER
    assert_eq!(app_entry(KRX_OP_RENDER, &api_table, core::ptr::null()), 0);
    assert_eq!(unsafe { APP_STATE.rendered_frames }, 1);

    // 3. KEY EVENT (UP) -> Consumed (1)
    let ev_up = KrxEvent {
        kind: KRX_EV_KEY,
        key: 1,
        flags: 0,
        reserved: 0,
        payload: 0,
    };
    assert_eq!(
        app_entry(KRX_OP_EVENT, &api_table, &ev_up as *const _ as *const ()),
        1
    );
    assert_eq!(unsafe { APP_STATE.key_presses }, 1);

    // 4. KEY EVENT (EXIT) -> Pass to system (0)
    let ev_exit = KrxEvent {
        kind: KRX_EV_KEY,
        key: 15,
        flags: 0,
        reserved: 0,
        payload: 0,
    };
    assert_eq!(
        app_entry(KRX_OP_EVENT, &api_table, &ev_exit as *const _ as *const ()),
        0
    );
    assert_eq!(unsafe { APP_STATE.key_presses }, 2);

    // 5. EXIT
    assert_eq!(app_entry(KRX_OP_EXIT, &api_table, core::ptr::null()), 0);
    assert!(unsafe { APP_STATE.exited });

    // 6. High-Throughput Deterministic Dispatch Bench (10,000 rapid event & render cycles)
    unsafe {
        APP_STATE.exited = false;
    }
    for i in 0..10_000u32 {
        let tick_ev = KrxEvent {
            kind: KRX_EV_TICK,
            key: 0,
            flags: 0,
            reserved: 0,
            payload: i,
        };
        let res = app_entry(KRX_OP_EVENT, &api_table, &tick_ev as *const _ as *const ());
        assert_eq!(res, 0);

        if (i & 0x7F) == 0 {
            let r_res = app_entry(KRX_OP_RENDER, &api_table, core::ptr::null());
            assert_eq!(r_res, 0);
        }
    }
    assert_eq!(unsafe { APP_STATE.rendered_frames }, 1 + (10_000 / 128) + 1);
}

#[test]
fn test_exhaustive_abi_vtable_verification_bench() {
    use crate::proc::abi::*;
    use ::abi::Op;

    let api = KrxApiTable::safe_stub(0xFFFF_FFFF);

    unsafe {
        // SYS & MATH
        assert_eq!(api.invoke(Op::SysQuery, 0, 0, 0), KRX_ABI_VERSION as usize);
        assert_eq!(api.invoke(Op::SysQuery, 1, 0, 0), 1024);
        assert_eq!(api.invoke(Op::SysQuery, 2, 0, 0), 48_000_000);
        assert_eq!(api.invoke(Op::SysInfo, 0, 0, 0), 1);

        // PROC & IPC
        assert_eq!(api.invoke(Op::ProcLeaf, 0, 100, 0), 0xFFFF_FFFF);
        assert_eq!(api.invoke(Op::ProcSvc, 0 | (0x1234 << 8), 1, 0), 1);
        assert_eq!(api.invoke(Op::ProcSvc, 1 | (0x1234 << 8), 1, 0), 0);
        assert_eq!(api.invoke(Op::ProcCtl, 0, 0, 0), 1);
        assert_eq!(api.invoke(Op::IpcSend, 0, 0, 0), 1);
        assert_eq!(api.invoke(Op::IpcRecv, 0, 0, 0), 1);
        assert_eq!(api.invoke(Op::IpcPoll, 0, 16, 0), 0xFF);

        // FS
        assert_eq!(api.invoke(Op::FsRead, 0, 0, 0), 0);
        assert_eq!(api.invoke(Op::FsWrite, 0, 0, 0), 1);
        assert_eq!(api.invoke(Op::FsMeta, 2, 0, 0), 1);
        assert_eq!(api.invoke(Op::FsRename, 0, 0, 0), 1);

        // RF & AUD
        assert_eq!(api.invoke(Op::RfTune, 145_500_000, 1, 12_500), 1);
        assert_eq!(api.invoke(Op::RfGet, 0, 0, 0), 0);
        assert_eq!(api.invoke(Op::RfGet, 1, 0, 0) as u32 as i32, -140);
        assert_eq!(api.invoke(Op::RfSet, 6, 0, 0), 1);
        assert_eq!(api.invoke(Op::AudCtl, 3, 8000, 0), 1);

        // GFX & UI
        assert_eq!(api.invoke(Op::GfxClear, 0, 0, 0), 0);
        assert_eq!(api.invoke(Op::GfxDraw, 5 | (1 << 8), 0, b'A' as usize), 6);
        assert_eq!(api.invoke(Op::GfxCommit, 0, 0, 0), 0);
        assert_eq!(api.invoke(Op::UiWidget, 1, 0, 0), 1);

        // IO
        assert_eq!(api.invoke(Op::IoPower, 0, 0, 0), 4000);
        assert_eq!(api.invoke(Op::IoPower, 5, 0, 0), 250);
        assert_eq!(api.invoke(Op::IoTime, 3, 0, 0), 1);
        assert_eq!(api.invoke(Op::IoBus, 0, 0, 0), 1);
        assert_eq!(api.invoke(Op::IoBus, 6, 0, 0), 1);
    }
}

#[test]
fn test_paged_execution_with_shadow_stack_and_leafs_bench() {
    use super::paged::*;

    // 1. Verify ShadowReturnStack push/pop and nesting
    let mut stack = ShadowReturnStack::<4>::new();
    assert_eq!(stack.sp, 0);

    // Simulate nested paged calls:
    // Frame 1: caller in block 1 at offset 0x0100 calls callee in block 2, return offset 0x0120
    let frame1 = ReturnFrame {
        caller_block_id: 1,
        caller_slot: 0,
        return_offset: 0x0120,
    };
    assert!(stack.push(frame1).is_ok());
    assert_eq!(stack.sp, 1);

    // Frame 2: block 2 at offset 0x0040 calls callee in block 3, return offset 0x0060
    let frame2 = ReturnFrame {
        caller_block_id: 2,
        caller_slot: 1,
        return_offset: 0x0060,
    };
    assert!(stack.push(frame2).is_ok());
    assert_eq!(stack.sp, 2);

    // Callee in block 3 completes, executes paged_return() -> restores frame2
    let popped2 = stack.pop().expect("Pop frame 2");
    assert_eq!(popped2.caller_block_id, 2);
    assert_eq!(popped2.return_offset, 0x0060);
    assert_eq!(stack.sp, 1);

    // Callee in block 2 completes, executes paged_return() -> restores frame1
    let popped1 = stack.pop().expect("Pop frame 1");
    assert_eq!(popped1.caller_block_id, 1);
    assert_eq!(popped1.return_offset, 0x0120);
    assert_eq!(stack.sp, 0);

    // Underflow protection
    assert_eq!(stack.pop().unwrap_err(), PagedError::ShadowStackUnderflow);

    // Overflow protection
    for i in 0..4 {
        assert!(
            stack
                .push(ReturnFrame {
                    caller_block_id: i as u8,
                    caller_slot: 0,
                    return_offset: 0x10,
                })
                .is_ok()
        );
    }
    assert_eq!(
        stack
            .push(ReturnFrame {
                caller_block_id: 99,
                caller_slot: 0,
                return_offset: 0x10,
            })
            .unwrap_err(),
        PagedError::ShadowStackOverflow
    );

    // 2. StructuredLeafManager overlay dispatch and swapping
    let mut leaf_mgr = StructuredLeafManager::<4>::new(0x2000_1000, 1024);
    assert_eq!(leaf_mgr.leaf_count, 0);
    assert_eq!(leaf_mgr.total_swaps, 0);

    // Configure Leaf 0 (Math/DSP) and Leaf 1 (Protocol Packet Demod)
    leaf_mgr.leaves[0] = LeafEntry {
        leaf_id: 0,
        flags: 0,
        size: 256,
        flash_offset: 0x1000,
        entry_offset: 0x00,
        ram_scratch_bytes: 0,
        reserved: [0u8; 4],
    };
    leaf_mgr.leaves[1] = LeafEntry {
        leaf_id: 1,
        flags: 0,
        size: 512,
        flash_offset: 0x1800,
        entry_offset: 0x04,
        ram_scratch_bytes: 0,
        reserved: [0u8; 4],
    };
    leaf_mgr.leaf_count = 2;

    let mut storage_reads = 0u32;
    // Dispatch Leaf 0 -> Cache Miss (loads from storage)
    let res0 = leaf_mgr
        .dispatch_leaf_trampoline(0, 42, 8, |_entry, is_hit, _ctx| {
            if !is_hit {
                storage_reads += 1;
            }
            Ok(42 * 2)
        })
        .expect("Dispatch Leaf 0");
    assert_eq!(res0, 84);
    assert_eq!(storage_reads, 1);
    assert_eq!(leaf_mgr.active_leaf, Some(0));

    // Dispatch Leaf 0 again -> Cache Hit (no storage read!)
    let res0_hit = leaf_mgr
        .dispatch_leaf_trampoline(0, 10, 8, |_entry, is_hit, _ctx| {
            if !is_hit {
                storage_reads += 1;
            }
            Ok(10 * 2)
        })
        .expect("Dispatch Leaf 0 Hit");
    assert_eq!(res0_hit, 20);
    assert_eq!(storage_reads, 1); // Still 1, 0 additional reads!

    // Dispatch Leaf 1 -> Overlay Swap (loads Leaf 1 into window)
    let res1 = leaf_mgr
        .dispatch_leaf_trampoline(1, 100, 8, |_entry, is_hit, _ctx| {
            if !is_hit {
                storage_reads += 1;
            }
            Ok(100 + 7)
        })
        .expect("Dispatch Leaf 1");
    assert_eq!(res1, 107);
    assert_eq!(storage_reads, 2);
    assert_eq!(leaf_mgr.active_leaf, Some(1));
    assert_eq!(leaf_mgr.total_swaps, 2);
}

#[test]
fn test_streamed_pages_and_lru_cache_bench() {
    use super::paged::*;

    let mut cache = FixedWindowCache::<8, 2>::new();
    // Configure 4 uniform 1024-byte blocks across 2 cache slots
    cache.configure(1024, 2, 0x10000, 4096, None);
    assert_eq!(cache.active_slots, 2);
    assert_eq!(cache.block_count, 4);

    // Access Block 0 -> Miss, allocated to Slot 0
    let (slot0, hit0, _entry0) = cache.resolve_or_select_slot(0).expect("Alloc block 0");
    assert!(!hit0);
    assert_eq!(slot0, 0);
    assert_eq!(cache.fault_count, 1);
    assert_eq!(cache.hit_count, 0);

    // Access Block 1 -> Miss, allocated to Slot 1
    let (slot1, hit1, _entry1) = cache.resolve_or_select_slot(1).expect("Alloc block 1");
    assert!(!hit1);
    assert_eq!(slot1, 1);
    assert_eq!(cache.fault_count, 2);
    assert_eq!(cache.hit_count, 0);

    // Access Block 0 again -> Cache Hit!
    let (slot0_again, hit0_again, _entry0_again) =
        cache.resolve_or_select_slot(0).expect("Hit block 0");
    assert!(hit0_again);
    assert_eq!(slot0_again, 0);
    assert_eq!(cache.fault_count, 2);
    assert_eq!(cache.hit_count, 1);

    // Access Block 2 -> All slots full! Evicts least recently used (Block 1 in Slot 1)
    let (slot_evicted, hit_evicted, _entry2) =
        cache.resolve_or_select_slot(2).expect("Evict block 1");
    assert!(!hit_evicted);
    assert_eq!(slot_evicted, 1); // Evicted slot 1
    assert_eq!(cache.fault_count, 3);
    assert_eq!(cache.slots[1].resident_block, Some(2));
    assert_eq!(cache.slots[0].resident_block, Some(0)); // Block 0 kept
}

#[test]
fn test_deep_edge_case_ram_orchestration_and_process_stress_bench() {
    use super::paged::*;
    use crate::proc::*;
    use crate::sched::Priority;
    use crate::sys::MemoryPressure;
    use ::abi::CapabilitySet;

    // ══════════════════════════════════════════════════════════════════
    // 1. RAM ORCHESTRATION & MEMORY PRESSURE EDGE CASES
    // ══════════════════════════════════════════════════════════════════

    // A. Boundary limits on SectionExecution::can_execute
    let exec = SectionExecution {
        min_stack_bytes: 512,
        min_heap_bytes: 256,
        page_size: 1024,
        cache_blocks: 2,
        dma_burst_size: 4,
        entry_point: 0x2000_0001,
        payload_size: 1024,
        reloc_offset: 0,
        reloc_count: 0,
        got_offset: 0,
        rt_pin_mask: 0,
        max_fault_latency_us: 100,
        reserved: [0; 2],
    };

    // Stack margin required = 256 bytes -> Total needed = 512 + 256 + 256 = 1024 bytes
    assert!(!exec.can_execute(1023, 256), "1 byte under must fail");
    assert!(
        exec.can_execute(1024, 256),
        "Exact required RAM must succeed"
    );
    assert!(exec.can_execute(4096, 256), "Abundant RAM must succeed");
    assert!(!exec.can_execute(0, 0), "Zero RAM must fail");

    // Arithmetic overflow defense
    let huge_exec = SectionExecution {
        min_stack_bytes: u16::MAX,
        min_heap_bytes: u16::MAX,
        page_size: 0,
        cache_blocks: 0,
        dma_burst_size: 0,
        entry_point: 0,
        payload_size: 0,
        reloc_offset: 0,
        reloc_count: 0,
        got_offset: 0,
        rt_pin_mask: 0,
        max_fault_latency_us: 0,
        reserved: [0; 2],
    };
    assert!(!huge_exec.can_execute(1024, usize::MAX));

    // B. Memory Pressure State Machine Verification
    assert_eq!(MemoryPressure::Normal.as_str(), "NORM");
    assert_eq!(MemoryPressure::Moderate.as_str(), "MOD");
    assert_eq!(MemoryPressure::Constrained.as_str(), "WARN");
    assert_eq!(MemoryPressure::Critical.as_str(), "CRIT");

    // ══════════════════════════════════════════════════════════════════
    // 2. DEEP TASK MANAGING & SATURATED PROCESS TABLE STRESS
    // ══════════════════════════════════════════════════════════════════
    let mut table = ProcessTable::<8>::new();

    // Spawn 4 system critical daemons (PID <= 4, protected)
    let d1 = table
        .spawn(
            "RADIO_DAEMON",
            0xF1,
            Priority::Safety,
            ExecType::StaticFirmware,
            CapabilitySet::ALL,
        )
        .unwrap();
    let d2 = table
        .spawn(
            "POWER_MGR",
            0xF2,
            Priority::Safety,
            ExecType::StaticFirmware,
            CapabilitySet::ALL,
        )
        .unwrap();
    let d3 = table
        .spawn(
            "AUDIO_SRV",
            0xF3,
            Priority::Radio,
            ExecType::StaticFirmware,
            CapabilitySet::ALL,
        )
        .unwrap();
    let d4 = table
        .spawn(
            "STORAGE_SRV",
            0xF4,
            Priority::Capture,
            ExecType::StaticFirmware,
            CapabilitySet::ALL,
        )
        .unwrap();

    assert_eq!(d1.0, 1);
    assert_eq!(d2.0, 2);
    assert_eq!(d3.0, 3);
    assert_eq!(d4.0, 4);

    // Spawn 4 user processes to completely fill the 8 slots
    let u1 = table
        .spawn(
            "VFO_APP",
            1,
            Priority::Ui,
            ExecType::RelocatableNative,
            CapabilitySet::NONE,
        )
        .unwrap();
    let u2 = table
        .spawn(
            "SETTINGS",
            2,
            Priority::Ui,
            ExecType::RelocatableNative,
            CapabilitySet::NONE,
        )
        .unwrap();
    let u3 = table
        .spawn(
            "BEACON_KRX",
            3,
            Priority::Idle,
            ExecType::RelocatableNative,
            CapabilitySet::NONE,
        )
        .unwrap();
    let u4 = table
        .spawn(
            "BENCH_KRX",
            4,
            Priority::Idle,
            ExecType::RelocatableNative,
            CapabilitySet::NONE,
        )
        .unwrap();

    assert_eq!(u4.0, 8);
    assert!(
        table
            .spawn(
                "FAIL_APP",
                5,
                Priority::Idle,
                ExecType::RelocatableNative,
                CapabilitySet::NONE
            )
            .is_err(),
        "Table must be full"
    );

    // Suspend user apps with distinct CPU tick counts
    if let Some(pcb) = table.find_mut(u1) {
        pcb.set_state(ProcState::Suspended);
        pcb.ledger.cpu_ticks = 500;
    }
    if let Some(pcb) = table.find_mut(u2) {
        pcb.set_state(ProcState::Suspended);
        pcb.ledger.cpu_ticks = 300; // Victim 2
    }
    if let Some(pcb) = table.find_mut(u3) {
        pcb.set_state(ProcState::Suspended);
        pcb.ledger.cpu_ticks = 100; // Victim 1 (lowest CPU ticks)
    }
    if let Some(pcb) = table.find_mut(u4) {
        pcb.set_state(ProcState::Running); // Cannot evict running app
        pcb.ledger.cpu_ticks = 50;
    }

    // Test spawn_or_recycle under extreme saturation
    // Must recycle u3 (lowest CPU ticks among suspended user apps: 100 ticks)
    let recycled1 = table
        .spawn_or_recycle(
            "STREAM_P1",
            5,
            Priority::Ui,
            ExecType::RelocatableNative,
            CapabilitySet::NONE,
        )
        .unwrap();
    assert!(
        table.find(u3).is_none(),
        "Lowest tick suspended process u3 must be evicted"
    );
    assert_eq!(table.find(recycled1).unwrap().name, "STREAM_P1");

    // Next victim must be u2 (300 ticks)
    let recycled2 = table
        .spawn_or_recycle(
            "LEAF_P0",
            6,
            Priority::Ui,
            ExecType::RelocatableNative,
            CapabilitySet::NONE,
        )
        .unwrap();
    assert!(table.find(u2).is_none(), "u2 must be evicted next");
    assert_eq!(table.find(recycled2).unwrap().name, "LEAF_P0");

    // System daemons (PID 1..4) must NEVER be evicted under any circumstances
    for pid_val in 1..=4 {
        assert!(
            table.find(Pid(pid_val)).is_some(),
            "Critical daemon PID {} must survive saturation eviction",
            pid_val
        );
    }

    // ══════════════════════════════════════════════════════════════════
    // 3. DEMAND-PAGED STREAMING & LRU CACHE THRASHING STRESS
    // ══════════════════════════════════════════════════════════════════
    let mut cache = FixedWindowCache::<16, 2>::new();
    // Setup 16 blocks: Block 0 has BLOCK_FLAG_PINNED (1)
    let mut block_map_bytes = [0u8; 16 * BlockEntry::SIZE];
    for b in 0..16 {
        let offset = b * BlockEntry::SIZE;
        block_map_bytes[offset] = b as u8; // block_id
        block_map_bytes[offset + 1] = if b == 0 { BLOCK_FLAG_PINNED } else { 0 }; // flags
        block_map_bytes[offset + 2..offset + 4].copy_from_slice(&1024u16.to_le_bytes()); // size = 1024
        block_map_bytes[offset + 4..offset + 8].copy_from_slice(&((b * 1024) as u32).to_le_bytes()); // flash_offset
    }
    cache.configure(1024, 2, 0, 16384, Some(&block_map_bytes));
    assert_eq!(cache.active_slots, 2);
    assert_eq!(cache.block_count, 16);

    // Resolve Block 0 -> Miss, allocated to Slot 0 (pinned!)
    let (s0, hit0, entry0) = cache.resolve_or_select_slot(0).unwrap();
    assert_eq!(s0, 0);
    assert!(!hit0);
    assert!(entry0.is_pinned());
    assert!(cache.slots[0].pinned);

    // Stream blocks 1 through 15 sequentially through the single remaining unpinned slot (Slot 1)
    for blk in 1..=15 {
        let (slot, hit, _) = cache.resolve_or_select_slot(blk).unwrap();
        assert_eq!(
            slot, 1,
            "Pinned Slot 0 must NEVER be evicted; Block {} must use Slot 1",
            blk
        );
        assert!(!hit);
        assert_eq!(
            cache.slots[0].resident_block,
            Some(0),
            "Slot 0 must remain pinned to Block 0"
        );
    }

    // Verify hit on pinned block 0
    let (s0_hit, hit0_again, _) = cache.resolve_or_select_slot(0).unwrap();
    assert_eq!(s0_hit, 0);
    assert!(hit0_again, "Pinned Block 0 must be a cache hit");

    // ══════════════════════════════════════════════════════════════════
    // 4. DEEP SHADOW RETURN STACK NESTING STRESS
    // ══════════════════════════════════════════════════════════════════
    let mut shadow = ShadowReturnStack::<4>::new();
    for i in 0..4 {
        assert!(
            shadow
                .push(ReturnFrame {
                    caller_block_id: i,
                    caller_slot: (i % 2) as u8,
                    return_offset: (i * 0x40) as u16,
                })
                .is_ok()
        );
    }
    assert_eq!(
        shadow.push(ReturnFrame {
            caller_block_id: 99,
            caller_slot: 0,
            return_offset: 0
        }),
        Err(PagedError::ShadowStackOverflow)
    );

    for i in (0..4).rev() {
        let frame = shadow.pop().unwrap();
        assert_eq!(frame.caller_block_id, i);
        assert_eq!(frame.return_offset, (i * 0x40) as u16);
    }
    assert_eq!(shadow.pop(), Err(PagedError::ShadowStackUnderflow));

    // ══════════════════════════════════════════════════════════════════
    // 5. STRUCTURED LEAF MULTI-HOP TRAMPOLINE CHAIN STRESS
    // ══════════════════════════════════════════════════════════════════
    let mut leaf_mgr = StructuredLeafManager::<8>::new(0x2000_1000, 1024);
    // Setup synthetic leaf map: Leaf 1..4 (16 bytes per LeafEntry)
    let leaf_map_bytes = [
        1, 0, 0x80, 0x00, 0x10, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
        0, // Leaf 1: offset 0x10, size 128
        2, 0, 0x80, 0x00, 0x90, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
        0, // Leaf 2: offset 0x90, size 128
        3, 0, 0x80, 0x00, 0x10, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
        0, // Leaf 3: offset 0x110, size 128
        4, 0, 0x80, 0x00, 0x90, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
        0, // Leaf 4: offset 0x190, size 128
    ];
    leaf_mgr.configure(0x2000_1000, 1024, Some(&leaf_map_bytes));
    assert_eq!(leaf_mgr.leaf_count, 4);

    // Verify dispatch and address resolution
    let (entry1, hit1) = leaf_mgr.begin_leaf_call(1).unwrap();
    let flash_offset1 = entry1.flash_offset;
    assert_eq!(flash_offset1, 0x10);
    assert!(!hit1);
    let cont = leaf_mgr
        .end_leaf_call(LeafContinuation::call_leaf(2, 52))
        .unwrap();
    assert!(cont.is_call_leaf());

    let (entry2, hit2) = leaf_mgr.begin_leaf_call(2).unwrap();
    let flash_offset2 = entry2.flash_offset;
    assert_eq!(flash_offset2, 0x90);
    assert!(!hit2);
    let cont2 = leaf_mgr.end_leaf_call(LeafContinuation::done(104)).unwrap();
    assert!(cont2.is_done());
    assert_eq!(cont2.payload, 104);

    assert_eq!(
        leaf_mgr.begin_leaf_call(99).unwrap_err(),
        PagedError::InvalidLeafId
    );
}

#[test]
fn test_krx_multi_type_lifecycle_and_flags() {
    use ::abi::Cap;

    // Construct synthetic Daemon KRX binary package in memory
    let mut pkg = [0u8; 128];

    // 1. Preamble (24 bytes)
    pkg[0..4].copy_from_slice(&KRX_IDENT);
    pkg[4] = KrxClass::Native as u8;
    pkg[5] = 5; // format_version
    pkg[6..8].copy_from_slice(&3u16.to_le_bytes()); // section_count = 3
    pkg[8..10].copy_from_slice(&124u16.to_le_bytes()); // header_len = 124
    pkg[10..12].copy_from_slice(&0u16.to_le_bytes()); // flags = 0
    pkg[12..16].copy_from_slice(&0u32.to_le_bytes()); // payload_crc32

    // Sections layout:
    // Offset 24: SectionIdentity (4B header + 40B body = 44B, ends at 68)
    pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
    pkg[26..28].copy_from_slice(&44u16.to_le_bytes());
    pkg[28..30].copy_from_slice(&108u16.to_le_bytes()); // name_str offset = 108
    pkg[30] = 1; // version major
    pkg[33..35].copy_from_slice(&::abi::VERSION.to_le_bytes()); // built-against ABI
    pkg[51] = 7; // icon_id = 7

    // Offset 68: SectionTypeLifecycle (4B header + 12B body = 16B, ends at 84)
    let flags =
        SectionType::FLAG_STARTUP | SectionType::FLAG_PERSISTENT | SectionType::FLAG_SINGLETON;
    let daemon_caps = (Cap::RadioRx as u32) | (Cap::AudioOutput as u32) | (Cap::SerialComm as u32);
    pkg[68..70].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
    pkg[70..72].copy_from_slice(&16u16.to_le_bytes());
    pkg[72] = KrxExecType::Daemon as u8;
    pkg[73] = flags;
    pkg[74] = 2; // stack_cost
    pkg[80..84].copy_from_slice(&daemon_caps.to_le_bytes());

    // Offset 84: SectionExecution (4B header + 28B body = 32B, ends at 116)
    pkg[84..86].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
    pkg[86..88].copy_from_slice(&32u16.to_le_bytes());
    pkg[88..90].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes

    // Offset 116: String Pool ("AprsDaemon\0")
    pkg[28..30].copy_from_slice(&116u16.to_le_bytes()); // name_str offset = 116
    let name_bytes = b"AprsDaemon\0";
    pkg[116..116 + name_bytes.len()].copy_from_slice(name_bytes);

    // Header total length: 128 bytes (padded to 4-byte boundary)
    pkg[8..10].copy_from_slice(&128u16.to_le_bytes()); // header_len = 128

    // Compute IEEE 802.3 CRC32 of header content (pkg[24..128])
    let header_crc = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
    pkg[16..20].copy_from_slice(&header_crc.to_le_bytes());

    // Parse and summarize
    let summary = KrxPreamble::summarize(&pkg[..128]).expect("Valid summary");
    assert_eq!(summary.preamble.app_type, KrxClass::Native as u8);
    assert_eq!(summary.title(), "AprsDaemon");
    assert_eq!(summary.abi, ::abi::VERSION);
    let exec = summary.exec().copied().expect("Has exec section");
    let min_stack = exec.min_stack_bytes;
    assert_eq!(min_stack, 256);
    assert_eq!(summary.exec_type, KrxExecType::Daemon);
    assert_eq!(summary.type_flags, flags);
    assert!(summary.is_startup());
    assert!(summary.is_persistent());
    assert!(summary.is_singleton());
    assert_eq!(summary.icon_id, 7);

    let caps = summary.caps;
    assert!(caps.contains(Cap::RadioRx));
    assert!(caps.contains(Cap::AudioOutput));
    assert!(caps.contains(Cap::SerialComm));
    assert!(!caps.contains(Cap::DisplayDirect));

    // Test Corrupted Header CRC rejection
    pkg[16] ^= 0xFF;
    assert!(KrxPreamble::summarize(&pkg[..128]).is_err());
    pkg[16] ^= 0xFF; // Restore

    // Test Service Exec Type
    pkg[72] = KrxExecType::Service as u8;
    let header_crc_svc = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
    pkg[16..20].copy_from_slice(&header_crc_svc.to_le_bytes());
    let summary_svc = KrxPreamble::summarize(&pkg[..128]).expect("Valid service summary");
    assert_eq!(summary_svc.exec_type, KrxExecType::Service);

    // Test Driver Exec Type
    pkg[72] = KrxExecType::Driver as u8;
    let header_crc_drv = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
    pkg[16..20].copy_from_slice(&header_crc_drv.to_le_bytes());
    let summary_drv = KrxPreamble::summarize(&pkg[..128]).expect("Valid driver summary");
    assert_eq!(summary_drv.exec_type, KrxExecType::Driver);

    // Test Module Exec Type
    pkg[72] = KrxExecType::Module as u8;
    let header_crc_mod = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
    pkg[16..20].copy_from_slice(&header_crc_mod.to_le_bytes());
    let summary_mod = KrxPreamble::summarize(&pkg[..128]).expect("Valid module summary");
    assert_eq!(summary_mod.exec_type, KrxExecType::Module);
}

#[test]
fn test_section_actions_parse() {
    let mut body = [0u8; 32];
    body[0..2].copy_from_slice(&1u16.to_le_bytes());
    body[2] = ::abi::scope::SHORTCUT | ::abi::scope::DRAWER;
    body[4..9].copy_from_slice(b"Sched");

    body[16..18].copy_from_slice(&2u16.to_le_bytes());
    body[18] = ::abi::scope::SHORTCUT;
    body[20..25].copy_from_slice(b"IpcLv");

    assert_eq!(KrxActionDecl::count(&body), 2);
    let a0 = KrxActionDecl::parse(&body, 0).unwrap();
    assert_eq!(a0.action_id, 1);
    assert_eq!(a0.label_str(), "Sched");
    let a1 = KrxActionDecl::parse(&body, 1).unwrap();
    assert_eq!(a1.action_id, 2);
    assert_eq!(a1.label_str(), "IpcLv");
    assert!(KrxActionDecl::parse(&body, 2).is_none());
}

#[test]
fn test_section_config_parse_and_strings() {
    let mut body = [0u8; 32];
    body[0..2].copy_from_slice(&1u16.to_le_bytes()); // version
    body[2] = 1; // item_count
    body[3] = 16; // cfg_bytes
    body[16] = 1; // offset
    body[17] = CFG_KIND_CHOICE;
    body[19] = 1; // step
    body[20..22].copy_from_slice(&1i16.to_le_bytes());
    body[24..26].copy_from_slice(&2i16.to_le_bytes());
    body[26..28].copy_from_slice(&64u16.to_le_bytes());
    body[28..30].copy_from_slice(&70u16.to_le_bytes());

    let hdr_info = KrxConfigHeader::parse(&body).unwrap();
    assert_eq!(hdr_info.version, 1);
    assert_eq!(hdr_info.cfg_bytes, 16);
    let item = KrxConfigItem::parse(&body, 0).unwrap();
    assert_eq!(item.kind, CFG_KIND_CHOICE);
    assert_eq!(item.max_v, 2);
    assert!(KrxConfigItem::parse(&body, 1).is_none());

    let mut pool = [0u8; 96];
    pool[64..68].copy_from_slice(b"Tab\0");
    pool[70..83].copy_from_slice(b"Proc\0Sch\0Kr\0\0");
    assert_eq!(KrxPreamble::read_hdr_str(&pool, 64), Some("Tab"));
    assert_eq!(
        KrxPreamble::read_hdr_packed(&pool, 70),
        Some("Proc\0Sch\0Kr\0")
    );
}

#[test]
fn fuzz_krx_summary_and_header_parsers() {
    let mut rng = 0xA1B2_C3D4u32;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng
    };

    let mut buf = [0u8; KrxPreamble::MAX_HEADER_SIZE + 16];
    for iter in 0..2000usize {
        let len = (next() as usize) % buf.len();
        for b in &mut buf[..len] {
            *b = next() as u8;
        }
        // Half the iterations craft a valid ident + plausible header_len & zero CRC
        // so the section walker, string scanner, and config/action parsers are deeply exercised.
        if (iter & 1) == 0 && len >= KrxPreamble::SIZE {
            buf[0..4].copy_from_slice(&KRX_IDENT);
            let h_len = (((next() as usize) % (len + 1)) & !3).max(KrxPreamble::SIZE);
            buf[8..10].copy_from_slice(&(h_len as u16).to_le_bytes());
            buf[16..20].copy_from_slice(&0u32.to_le_bytes()); // bypass header_crc32 to fuzz section parsing
        }

        let slice = &buf[..len];
        let mut slot = core::mem::MaybeUninit::<KrxSummary<'_>>::uninit();
        if let Ok(summary) = KrxPreamble::summarize_into(slice, &mut slot) {
            assert!(summary.preamble.header_len as usize >= KrxPreamble::SIZE);
            assert!(summary.preamble.header_len as usize <= len);
            if let Some(acts) = summary.aux.actions {
                let n = KrxActionDecl::count(acts);
                for k in 0..n.min(8) {
                    let decl = KrxActionDecl::parse(acts, k).unwrap();
                    let _ = decl.label_str();
                }
            }
            if let Some(cfg) = summary.aux.config {
                let _ = KrxConfigHeader::parse(cfg);
                for k in 0..4 {
                    let _ = KrxConfigItem::parse(cfg, k);
                }
            }
        }

        let off = (next() as u16) % (buf.len() as u16 + 16);
        let _ = KrxPreamble::read_hdr_str(slice, off);
        let _ = KrxPreamble::read_hdr_packed(slice, off);
    }
}
