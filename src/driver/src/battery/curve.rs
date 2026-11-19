//! High-Precision 2S Li-ion Battery Discharge Curves & Piecewise Interpolation.

/// Battery chemistry / capacity type profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryType {
    Type1600mAh = 0,
    Type2200mAh = 1,
    Type3500mAh = 2,
    Type1500mAhK1 = 3,
    Type2500mAhK1 = 4,
    TypeExternal = 5,
}

impl BatteryType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => BatteryType::Type1600mAh,
            1 => BatteryType::Type2200mAh,
            2 => BatteryType::Type3500mAh,
            3 => BatteryType::Type1500mAhK1,
            4 => BatteryType::Type2500mAhK1,
            5 => BatteryType::TypeExternal,
            _ => BatteryType::Type1600mAh,
        }
    }
}

/// Compact 4-byte discharge curve segment (voltage offset above 6.00V, base %, Q15 slope).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CurveSegment {
    pub v_off_10mv: u8,
    pub pct: u8,
    pub slope_q15: u16,
}

const fn seg(v_10mv: u16, pct: u8, slope_q16: u32) -> CurveSegment {
    CurveSegment {
        v_off_10mv: (v_10mv - 600) as u8,
        pct,
        slope_q15: ((slope_q16 + 1) >> 1) as u16,
    }
}

/// High-precision 2S Li-ion discharge curves (10mV resolution units -> Percentage 0..100)
/// Modeled after industry 2S Li-ion cell characteristics (8.4V max, 7.4V nominal, 6.0V cutoff).
#[repr(C)]
pub struct CurveDefinition {
    pub v_max: u16,
    pub segments: [CurveSegment; 6],
}

pub static VOLTAGE_CURVES: [CurveDefinition; 5] = [
    // 1600 mAh Standard Pack (UV-K5)
    CurveDefinition {
        v_max: 838,
        segments: [
            seg(820, 95, 18204),
            seg(780, 75, 32768),
            seg(745, 45, 56174),
            seg(715, 20, 54613),
            seg(670, 7, 18933),
            seg(600, 0, 6554),
        ],
    },
    // 2200 mAh Extended Pack
    CurveDefinition {
        v_max: 839,
        segments: [
            seg(825, 96, 18725),
            seg(785, 78, 29491),
            seg(750, 50, 52429),
            seg(720, 22, 61167),
            seg(680, 8, 22938),
            seg(600, 0, 6554),
        ],
    },
    // 3500 mAh High Capacity Pack
    CurveDefinition {
        v_max: 840,
        segments: [
            seg(828, 97, 16384),
            seg(790, 80, 29319),
            seg(755, 55, 46811),
            seg(725, 25, 65536),
            seg(685, 9, 26214),
            seg(600, 0, 6939),
        ],
    },
    // 1500 mAh Compact Pack (K1)
    CurveDefinition {
        v_max: 836,
        segments: [
            seg(818, 94, 21845),
            seg(778, 72, 36045),
            seg(742, 42, 54613),
            seg(712, 18, 52429),
            seg(668, 6, 17873),
            seg(600, 0, 5783),
        ],
    },
    // 2500 mAh Extended Pack (K1)
    CurveDefinition {
        v_max: 839,
        segments: [
            seg(826, 96, 20165),
            seg(788, 79, 29319),
            seg(752, 52, 49152),
            seg(722, 23, 63351),
            seg(682, 8, 24576),
            seg(600, 0, 6394),
        ],
    },
];

/// High-precision piecewise linear interpolation to convert OCV battery millivolts to percentage.
pub fn mv_to_percentage(mv: u16, bat_type: BatteryType) -> u8 {
    if bat_type == BatteryType::TypeExternal {
        return 100;
    }
    // Fast reciprocal division: div10_u16
    let (v_10mv, _) = utils::math::div10_u16(mv);
    let curve = &VOLTAGE_CURVES[bat_type as usize];

    if v_10mv >= curve.v_max {
        return 100;
    }
    if v_10mv < 600 {
        return 0;
    }

    let v_off = v_10mv - 600;
    for seg in &curve.segments {
        let s_off = seg.v_off_10mv as u16;
        if v_off >= s_off {
            let v_diff = (v_off - s_off) as u32;
            let interpolated = (seg.pct as u32) + ((v_diff * (seg.slope_q15 as u32)) >> 15);
            return (interpolated.min(100)) as u8;
        }
    }

    0
}
